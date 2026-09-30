--!nonstrict
-- Turns `toge` CLI results into launcher rows. Kept free of host globals so it
-- can be exercised by tests/results_test.luau with a stub `tr` and JSON decoder.

local M = {}

-- Exit codes documented by `toge --help` / toge/src/main.rs.
M.EXIT_OK = 0
M.EXIT_BAD_ARGS = 2
M.EXIT_NO_RESULTS = 9
M.EXIT_NOT_READY = 10
M.EXIT_NOT_FOUND = 127

-- First toge release with --json, --no-wait and `--`.
M.MIN_TOGE_VERSION = "0.3.0"

local EXT_GLYPHS = {
    pdf = "file-type-pdf",
    txt = "file-text",
    md = "file-text",
    doc = "file-word",
    docx = "file-word",
    odt = "file-word",
    xls = "file-spreadsheet",
    xlsx = "file-spreadsheet",
    ods = "file-spreadsheet",
    csv = "file-spreadsheet",
    zip = "file-zip",
    tar = "file-zip",
    gz = "file-zip",
    xz = "file-zip",
    zst = "file-zip",
    ["7z"] = "file-zip",
    png = "photo",
    jpg = "photo",
    jpeg = "photo",
    gif = "photo",
    webp = "photo",
    svg = "photo",
    mp3 = "music",
    flac = "music",
    ogg = "music",
    wav = "music",
    mp4 = "movie",
    mkv = "movie",
    webm = "movie",
    mov = "movie",
    rs = "file-code",
    lua = "file-code",
    luau = "file-code",
    py = "file-code",
    js = "file-code",
    ts = "file-code",
    c = "file-code",
    h = "file-code",
    cpp = "file-code",
    go = "file-code",
    sh = "file-code",
    toml = "file-code",
    json = "file-code",
    yaml = "file-code",
    yml = "file-code",
}

function M.glyphFor(entry)
    if entry.is_dir then
        return "folder"
    end
    return EXT_GLYPHS[string.lower(entry.ext or "")] or "file"
end

-- Replace a leading $HOME with ~ so subtitles stay short.
function M.shortenHome(path, home)
    if home == nil or home == "" or path == nil then
        return path
    end
    if path == home then
        return "~"
    end
    if string.sub(path, 1, #home + 1) == home .. "/" then
        return "~" .. string.sub(path, #home + 1)
    end
    return path
end

local function firstLine(text)
    if text == nil then
        return ""
    end
    local line = string.match(text, "[^\n]+") or ""
    -- The CLI prefixes its own errors; the row title already says it failed.
    line = string.gsub(line, "^toge: ", "")
    line = string.gsub(line, "^query failed: ", "")
    return line
end

-- Older toge releases reject the flags this plugin relies on (--json,
-- --no-wait, --) with exit code 2 and "unknown flag".
function M.isOutdated(result)
    return result.exitCode == M.EXIT_BAD_ARGS and string.find(result.stderr or "", "unknown flag", 1, true) ~= nil
end

local function outdatedRow(tr)
    return {
        id = "noop",
        title = tr("launcher.outdated"),
        subtitle = tr("launcher.outdated_subtitle", { version = M.MIN_TOGE_VERSION }),
        glyph = "alert-triangle",
    }
end

local function infoRow(id, title, subtitle, glyph)
    return { id = id, title = title, subtitle = subtitle, glyph = glyph or "info-circle" }
end

-- Rows for one `toge --json` run.
--   result: CommandResult from noctalia.runAsync
--   ctx:    { tr = fn(key, subst), decode = fn(str) -> value?, home = string? }
-- Returns rows, notReady (true when the daemon is still loading or indexing).
function M.fromCommand(result, ctx)
    local tr = ctx.tr
    if result.timedOut then
        return { infoRow("noop", tr("launcher.timed_out"), nil, "alert-triangle") }, false
    end

    local code = result.exitCode
    if code == M.EXIT_NOT_READY then
        local status = ctx.decode(firstLine(result.stdout))
        local message = status and status.message or nil
        return { infoRow("noop", tr("launcher.indexing"), message, "loader") }, true
    end
    if code == M.EXIT_NO_RESULTS then
        return { infoRow("noop", tr("launcher.no_results")) }, false
    end
    if code == M.EXIT_NOT_FOUND then
        return { infoRow("noop", tr("launcher.missing"), tr("launcher.missing_subtitle"), "alert-triangle") }, false
    end
    if M.isOutdated(result) then
        return { outdatedRow(tr) }, false
    end
    if code ~= M.EXIT_OK then
        return { infoRow("noop", tr("launcher.error"), firstLine(result.stderr), "alert-triangle") }, false
    end

    local entries = {}
    for line in string.gmatch(result.stdout or "", "[^\n]+") do
        -- A truncated final line fails to decode and is dropped.
        local entry = ctx.decode(line)
        if type(entry) == "table" and type(entry.path) == "string" then
            table.insert(entries, entry)
        end
    end

    local rows = {}
    for i, entry in ipairs(entries) do
        table.insert(rows, {
            id = "open:" .. entry.path,
            title = entry.name ~= "" and entry.name or entry.path,
            subtitle = M.shortenHome(entry.parent, ctx.home),
            glyph = M.glyphFor(entry),
            -- toge already ranked the matches; keep that order.
            score = #entries - i + 1,
        })
    end
    if #rows == 0 then
        table.insert(rows, infoRow("noop", tr("launcher.no_results")))
    end
    return rows, false
end

-- Status row for the empty query, from `toge --status --json`.
-- Returns row, notReady (true while the daemon is still loading or indexing).
function M.statusRow(result, ctx)
    local tr = ctx.tr
    if result.timedOut or result.exitCode ~= M.EXIT_OK then
        if result.exitCode == M.EXIT_NOT_FOUND then
            return infoRow("noop", tr("launcher.missing"), tr("launcher.missing_subtitle"), "alert-triangle"), false
        end
        if M.isOutdated(result) then
            return outdatedRow(tr), false
        end
        return infoRow("noop", tr("launcher.error"), firstLine(result.stderr), "alert-triangle"), false
    end
    local status = ctx.decode(firstLine(result.stdout))
    if type(status) ~= "table" then
        return infoRow("noop", tr("launcher.error"), nil, "alert-triangle"), false
    end
    if status.ready then
        local subtitle = tr("launcher.status_ready", { count = status.indexed_count })
        return infoRow("noop", tr("launcher.status_title"), subtitle, "database"), false
    end
    return infoRow("noop", tr("launcher.indexing"), status.message, "loader"), true
end

return M
