-- grape.toml detection and a minimal TOML parser tailored to the
-- grape.toml schema ([project] table + [dependencies] inline-table values).
local M = {}

-- Minimal TOML value parser. Supports the subset used by grape.toml:
--   - basic strings "..."
--   - literal strings '...'
--   - integers, floats, booleans
--   - arrays [...]
--   - inline tables { k = v, ... }
-- Returns (value, rest). Raises an error on malformed input.
local function parse_value(s)
    s = s:gsub("^%s+", "")
    -- basic string
    if s:sub(1, 1) == '"' then
        local rest = s:sub(2)
        local out = {}
        local i = 1
        while i <= #rest do
            local c = rest:sub(i, i)
            if c == "\\" then
                local next_c = rest:sub(i + 1, i + 1)
                if next_c == "n" then table.insert(out, "\n")
                elseif next_c == "t" then table.insert(out, "\t")
                elseif next_c == "r" then table.insert(out, "\r")
                elseif next_c == '"' then table.insert(out, '"')
                elseif next_c == "\\" then table.insert(out, "\\")
                else table.insert(out, next_c) end
                i = i + 2
            elseif c == '"' then
                return table.concat(out), rest:sub(i + 1)
            else
                table.insert(out, c)
                i = i + 1
            end
        end
        error("unterminated string in TOML")
    end
    -- literal string
    if s:sub(1, 1) == "'" then
        local end_pos = s:find("'", 2, true)
        if not end_pos then error("unterminated literal string in TOML") end
        return s:sub(2, end_pos - 1), s:sub(end_pos + 1)
    end
    -- array
    if s:sub(1, 1) == "[" then
        local arr = {}
        local rest = s:sub(2):gsub("^%s+", "")
        while rest:sub(1, 1) ~= "]" do
            local v
            v, rest = parse_value(rest)
            table.insert(arr, v)
            rest = rest:gsub("^%s+", "")
            if rest:sub(1, 1) == "," then
                rest = rest:sub(2):gsub("^%s+", "")
            end
        end
        return arr, rest:sub(2)
    end
    -- inline table
    if s:sub(1, 1) == "{" then
        local tbl = {}
        local rest = s:sub(2):gsub("^%s+", "")
        while rest:sub(1, 1) ~= "}" do
            local eq = rest:find("=", 1, true)
            if not eq then error("malformed inline table in TOML") end
            local key = rest:sub(1, eq - 1):gsub("^%s+", ""):gsub("%s+$", "")
            rest = rest:sub(eq + 1):gsub("^%s+", "")
            local v
            v, rest = parse_value(rest)
            tbl[key] = v
            rest = rest:gsub("^%s+", "")
            if rest:sub(1, 1) == "," then
                rest = rest:sub(2):gsub("^%s+", "")
            end
        end
        return tbl, rest:sub(2)
    end
    -- boolean / number / bare scalar
    local match = s:match("^([%w%.%+%-]+)")
    if not match then error("cannot parse TOML value: " .. s:sub(1, 20)) end
    local rest = s:sub(#match + 1)
    if match == "true" then return true, rest end
    if match == "false" then return false, rest end
    if match:find("%.") then
        return tonumber(match), rest
    end
    return tonumber(match) or match, rest
end

-- Parse a complete (small) TOML document into a nested table.
function M.parse_toml(content)
    local root = {}
    local current = root
    for raw_line in content:gmatch("([^\r\n]*)\r?\n?") do
        local line = raw_line:gsub("^%s+", "")
        -- strip trailing comments (naive: only outside strings; fine for grape.toml)
        line = line:gsub("%s*#.*$", "")
        if line ~= "" then
            -- table header [section] or [a.b]
            local header = line:match("^%[(.+)%]$")
            if header then
                local node = root
                for part in header:gmatch("([^%.]+)") do
                    part = part:gsub("^%s+", ""):gsub("%s+$", "")
                    -- strip quotes from quoted keys
                    part = part:gsub('^"(.+)"$', "%1"):gsub("^'(.+)'$", "%1")
                    node[part] = node[part] or {}
                    node = node[part]
                end
                current = node
            else
                local eq = line:find("=", 1, true)
                if eq then
                    local key = line:sub(1, eq - 1):gsub("^%s+", ""):gsub("%s+$", "")
                    key = key:gsub('^"(.+)"$', "%1"):gsub("^'(.+)'$", "%1")
                    local val_str = line:sub(eq + 1)
                    local value = parse_value(val_str)
                    current[key] = value
                end
            end
        end
    end
    return root
end

--- Find grape.toml by walking up from `start_dir` to the filesystem root.
--- Returns the absolute path or nil.
function M.find_grape_toml(start_dir)
    local dir = vim.fn.fnamemodify(start_dir, ":p")
    local visited = {}
    while dir and dir ~= "" and not visited[dir] do
        visited[dir] = true
        local candidate = dir .. "grape.toml"
        if vim.fn.filereadable(candidate) == 1 then
            return candidate
        end
        local parent = vim.fn.fnamemodify(dir, ":h")
        if parent == dir then break end
        dir = parent .. "/"
    end
    return nil
end

--- Read and parse a grape.toml file. Returns a normalized project table:
---   { name, version, entry, authors, description, license, dependencies = {...} }
--- or nil + error message.
function M.load_project(toml_path)
    local content = table.concat(vim.fn.readfile(toml_path), "\n")
    local ok, data = pcall(M.parse_toml, content)
    if not ok then
        return nil, "failed to parse " .. toml_path .. ": " .. data
    end
    local proj = data.project or {}
    local deps = data.dependencies or {}
    local result = {
        root = vim.fn.fnamemodify(toml_path, ":h"),
        toml_path = toml_path,
        name = proj.name or "(unnamed)",
        version = proj.version or "0.0.0",
        entry = proj.entry or "main.gbl",
        authors = proj.authors or {},
        description = proj.description,
        license = proj.license,
        dependencies = {},
    }
    for name, spec in pairs(deps) do
        if type(spec) == "table" then
            result.dependencies[name] = {
                repo = spec.repo or "",
                tag = spec.tag or "",
                optional = spec.optional or false,
            }
        end
    end
    return result, nil
end

return M
