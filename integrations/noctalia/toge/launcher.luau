--!nonstrict
-- toge — launcher provider. `/toge <text>` asks the toge daemon for matches
-- through the `toge` CLI and lists them; activating a row opens it with
-- xdg-open (directories open in the file manager).
--
-- One search runs at a time. Keystrokes that arrive meanwhile only replace
-- `pendingQuery`, and the finished search starts the latest one, so a slow
-- query never queues a backlog. setResults echoes the query it answers, which
-- lets the host drop anything that is no longer current.
--
-- `toge --no-wait` exits with code 10 while the daemon is still loading or
-- indexing instead of blocking for up to 30 seconds; the provider then shows
-- the daemon's progress message and retries once a second.

local results = require("./results.luau")

local SEARCH_TIMEOUT_MS = 10000
local STATUS_TIMEOUT_MS = 8000
local RETRY_LIMIT = 120

local searching = false
local pendingQuery = nil
local latestQuery = nil
local retryQuery = nil
local retries = 0

local search
local showStatus

local function tr(key, subst)
    return noctalia.tr(key, subst)
end

local function ctx()
    return {
        tr = tr,
        decode = function(text)
            local value = noctalia.json.decode(text)
            return value
        end,
        home = noctalia.getenv("HOME"),
    }
end

local function configString(key, fallback)
    local value = noctalia.getConfig(key)
    if type(value) ~= "string" or noctalia.string.trim(value) == "" then
        return fallback
    end
    return noctalia.expandPath(noctalia.string.trim(value))
end

local function togeCommand()
    return configString("toge_command", "toge")
end

local function maxResults()
    local value = tonumber(noctalia.getConfig("max_results")) or 20
    return math.clamp(math.floor(value), 5, 100)
end

local function utilityRows()
    local rows = {
        {
            id = "noop",
            title = tr("launcher.hint_title"),
            subtitle = tr("launcher.hint_subtitle"),
            glyph = "search",
        },
        {
            id = "reindex",
            title = tr("launcher.reindex_title"),
            subtitle = tr("launcher.reindex_subtitle"),
            glyph = "refresh",
        },
    }
    local gui = configString("gui_command", "toge-slint")
    if noctalia.commandExists(gui) then
        table.insert(rows, {
            id = "gui",
            title = tr("launcher.gui_title"),
            subtitle = tr("launcher.gui_subtitle"),
            glyph = "app-window",
        })
    end
    return rows
end

-- Re-run `query` in a second if it is still what the user is looking at.
-- The launcher has no timer API, so `sleep 1` stands in for one.
local function scheduleRetry(query, rerun)
    if retryQuery == query or retries >= RETRY_LIMIT then
        return
    end
    retryQuery = query
    retries += 1
    noctalia.runAsync({ "sleep", "1" }, function()
        retryQuery = nil
        if latestQuery == query then
            rerun(query)
        end
    end, 3000)
end

showStatus = function(query, previousRow)
    local rows = utilityRows()
    local function publish(statusRow)
        local all = { statusRow }
        for _, row in ipairs(rows) do
            table.insert(all, row)
        end
        launcher.setResults(query, all)
    end
    if previousRow ~= nil then
        publish(previousRow)
    else
        launcher.setResults(query, rows)
    end
    noctalia.runAsync({ togeCommand(), "--status", "--json" }, function(result)
        if latestQuery ~= query then
            return
        end
        local row, notReady = results.statusRow(result, ctx())
        publish(row)
        if notReady then
            scheduleRetry(query, function(q)
                showStatus(q, row)
            end)
        else
            retries = 0
        end
    end, STATUS_TIMEOUT_MS)
end

search = function(query)
    if searching then
        pendingQuery = query
        return
    end
    local text = noctalia.string.trim(query)
    searching = true
    local argv = { togeCommand(), "--json", "--no-wait", "-n", tostring(maxResults()), "--", text }
    local started = noctalia.runAsync(argv, function(result)
        searching = false
        local rows, notReady = results.fromCommand(result, ctx())
        launcher.setResults(query, rows)

        local nextQuery = pendingQuery
        pendingQuery = nil
        if nextQuery ~= nil and nextQuery ~= query then
            search(nextQuery)
        elseif notReady then
            scheduleRetry(query, search)
        else
            retries = 0
        end
    end, SEARCH_TIMEOUT_MS)
    if not started then
        searching = false
        launcher.setResults(query, {
            { id = "noop", title = tr("launcher.missing"), subtitle = tr("launcher.missing_subtitle"), glyph = "alert-triangle" },
        })
    end
end

function onQuery(query)
    latestQuery = query
    if noctalia.string.trim(query) == "" then
        showStatus(query)
        return
    end
    search(query)
end

local function reindex()
    noctalia.runAsync({ togeCommand(), "--reindex" })
    noctalia.notify(tr("title"), tr("launcher.reindex_started"))
end

function onActivate(id)
    local path = string.match(id, "^open:(.+)$")
    if path ~= nil then
        noctalia.runAsync({ "xdg-open", path })
        return
    end
    if id == "reindex" then
        reindex()
        -- Keep the launcher open on the status view so progress is visible.
        launcher.setQuery("")
        return
    end
    if id == "gui" then
        noctalia.runAsync({ configString("gui_command", "toge-slint") })
        return
    end
    -- Informational rows keep the launcher open where it is.
    launcher.setQuery(latestQuery or "")
end

-- `noctalia msg plugin fishdaa/toge:launcher all reindex`
function onIpc(event, _payload)
    if event == "reindex" then
        reindex()
    end
end
