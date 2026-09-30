-- Unit tests for results.luau. The module sticks to the Lua 5.1 subset, so
-- any Lua 5.1+ interpreter runs this: `luajit tests/results_test.lua`.
local dir = (arg and arg[0] or ""):match("^(.*)/tests/") or "."
local results = dofile(dir .. "/results.luau")

local failures = 0
local function check(name, cond, detail)
    if cond then
        print("ok   " .. name)
    else
        failures = failures + 1
        print("FAIL " .. name .. (detail and (": " .. tostring(detail)) or ""))
    end
end

local fixtures = {
    ['{"path":"/home/u/docs/a.pdf","name":"a.pdf","parent":"/home/u/docs","ext":"pdf","is_dir":false}'] = {
        path = "/home/u/docs/a.pdf", name = "a.pdf", parent = "/home/u/docs", ext = "pdf", is_dir = false,
    },
    ['{"path":"/srv/src","name":"src","parent":"/srv","ext":"","is_dir":true}'] = {
        path = "/srv/src", name = "src", parent = "/srv", ext = "", is_dir = true,
    },
    ['{"status":"indexing","ready":false,"message":"Indexing 2/5: /home"}'] = {
        status = "indexing", ready = false, message = "Indexing 2/5: /home",
    },
    ['{"status":"ready","ready":true,"message":"","indexed_count":42}'] = {
        status = "ready", ready = true, message = "", indexed_count = 42,
    },
}
local ctx = {
    tr = function(key, subst)
        if subst and subst.count then
            return key .. "(" .. subst.count .. ")"
        end
        if subst and subst.version then
            return key .. "(" .. subst.version .. ")"
        end
        return key
    end,
    decode = function(text)
        return fixtures[text]
    end,
    home = "/home/u",
}

local function run(exitCode, stdout, stderr, timedOut)
    return { exitCode = exitCode, stdout = stdout or "", stderr = stderr or "", timedOut = timedOut or false }
end

do
    local out = '{"path":"/home/u/docs/a.pdf","name":"a.pdf","parent":"/home/u/docs","ext":"pdf","is_dir":false}\n'
        .. '{"path":"/srv/src","name":"src","parent":"/srv","ext":"","is_dir":true}\n'
        .. '{"path":"/trunc'
    local rows, notReady = results.fromCommand(run(0, out), ctx)
    check("rows parsed, truncated line dropped", #rows == 2, #rows)
    check("not ready flag off", notReady == false)
    check("open id carries path", rows[1].id == "open:/home/u/docs/a.pdf", rows[1].id)
    check("title is name", rows[1].title == "a.pdf")
    check("home shortened", rows[1].subtitle == "~/docs", rows[1].subtitle)
    check("pdf glyph", rows[1].glyph == "file-type-pdf", rows[1].glyph)
    check("folder glyph", rows[2].glyph == "folder")
    check("order kept by score", rows[1].score > rows[2].score)
end

do
    local rows, notReady = results.fromCommand(
        run(10, '{"status":"indexing","ready":false,"message":"Indexing 2/5: /home"}\n'), ctx)
    check("not ready row", rows[1].title == "launcher.indexing" and rows[1].subtitle == "Indexing 2/5: /home")
    check("not ready flag on", notReady == true)
end

do
    local rows = results.fromCommand(run(9), ctx)
    check("exit 9 is no results", rows[1].title == "launcher.no_results")
    rows = results.fromCommand(run(0, ""), ctx)
    check("empty output is no results", rows[1].title == "launcher.no_results")
    rows = results.fromCommand(run(127), ctx)
    check("missing binary", rows[1].title == "launcher.missing")
    rows = results.fromCommand(run(1, "", "query failed: invalid regex: (foo\n"), ctx)
    check("error subtitle strips prefix", rows[1].subtitle == "invalid regex: (foo", rows[1].subtitle)
    rows = results.fromCommand(run(2, "", "toge: unknown flag: --json\n"), ctx)
    check("old toge is reported as outdated", rows[1].title == "launcher.outdated", rows[1].title)
    check("outdated row names the minimum version", rows[1].subtitle == "launcher.outdated_subtitle(" .. results.MIN_TOGE_VERSION .. ")", rows[1].subtitle)
    rows = results.fromCommand(run(2, "", "toge: missing max-results value\n"), ctx)
    check("other bad-args errors stay generic", rows[1].title == "launcher.error")
    local statusOld = results.statusRow(run(2, "", "toge: unknown flag: --json\n"), ctx)
    check("status on old toge is outdated", statusOld.title == "launcher.outdated")
    rows = results.fromCommand(run(-1, "", "", true), ctx)
    check("timeout row", rows[1].title == "launcher.timed_out")
    for _, row in ipairs(rows) do
        check("info rows are inert", row.id == "noop")
    end
end

do
    local row, readyRetry = results.statusRow(run(0, '{"status":"ready","ready":true,"message":"","indexed_count":42}\n'), ctx)
    check("status ready row", row.subtitle == "launcher.status_ready(42)", row.subtitle)
    check("status ready no retry", readyRetry == false)
    local notReady
    row, notReady = results.statusRow(run(0, '{"status":"indexing","ready":false,"message":"Indexing 2/5: /home"}\n'), ctx)
    check("status indexing row", row.title == "launcher.indexing")
    check("status indexing flags retry", notReady == true)
end

check("shortenHome leaves other prefixes", results.shortenHome("/home/uu/x", "/home/u") == "/home/uu/x")
check("shortenHome exact home", results.shortenHome("/home/u", "/home/u") == "~")

if failures > 0 then
    print(failures .. " failure(s)")
    os.exit(1)
end
print("all passed")
