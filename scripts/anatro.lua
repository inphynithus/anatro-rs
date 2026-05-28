-- Anatro-rs mpv integration
-- Skips intros and outros using KV-FS cache data from anatro-rs.
--
-- Default Keybindings & Actions Reference:
--   - Ctrl+i  (anatro-toggle-intro)  : Toggle Skip Intro (Default: OFF / false)
--   - Ctrl+o  (anatro-toggle-outro)  : Toggle Skip Outro (Default: OFF / false)
--   - BS      (anatro-undo-skip)      : Undo last skip (returns to pre-skip timestamp)
--

local ffi = require("ffi")
local bit = require("bit")
local utils = require("mp.utils")
local msg = require("mp.msg")

-- Session state (disabled by default, toggled via keybindings)
local config = {
    skip_intro = false,   -- Auto-skip intro (toggled by Ctrl+i)
    skip_outro = false,   -- Auto-skip outro (toggled by Ctrl+o)
    cache_dir = nil,      -- Resolved at init from XDG_CONFIG_HOME
}

--- Computes FNV-1a 64-bit hash, returning a 16-char uppercase hex string.
--- Uses LuaJIT FFI uint64_t for correct wrapping arithmetic.
local function fnv1a_64_hex(str)
    local hash = ffi.new("uint64_t", 0xcbf29ce484222325ULL)
    local prime = ffi.new("uint64_t", 0x100000001b3ULL)

    for i = 1, #str do
        local byte = string.byte(str, i)
        local low = tonumber(hash % 256ULL)
        local xored = bit.bxor(low, byte)
        hash = hash - ffi.new("uint64_t", low) + ffi.new("uint64_t", xored)
        hash = hash * prime
    end

    return string.format("%016X", hash)
end

--- Resolves the KV-FS cache directory path.
local function resolve_cache_dir()
    local config_home = os.getenv("XDG_CONFIG_HOME")
    if not config_home or config_home == "" then
        local home = os.getenv("HOME")
        if home and home ~= "" then
            config_home = home .. "/.config"
        else
            config_home = ""
        end
    end
    if config_home ~= "" then
        return config_home .. "/anatro-rs/cache"
    end
    return nil
end

config.cache_dir = resolve_cache_dir()

local current_entry = nil
local last_position = nil  -- for undo/voltar

--- Formats seconds as MM:SS for OSD display.
local function format_time(seconds)
    local m = math.floor(seconds / 60)
    local s = math.floor(seconds % 60)
    return string.format("%02d:%02d", m, s)
end

--- Returns true if `pos` is within [start, start + duration].
local function in_range(pos, start, duration)
    return pos >= start and pos <= (start + duration)
end

--- Seeks past a segment and shows OSD. Returns true if seek was performed.
local function skip_segment(label, start, duration)
    local pos = mp.get_property_number("time-pos")
    if not pos then return false end

    local target = start + duration
    local vid_duration = mp.get_property_number("duration") or math.huge

    if target >= vid_duration then
        last_position = pos
        mp.osd_message("⏭ " .. label .. " → next", 3)
        mp.commandv("playlist-next", "weak")
        return true
    end

    last_position = pos
    mp.commandv("seek", tostring(target), "absolute", "exact")
    mp.osd_message("⏭ " .. label .. " skipped → " .. format_time(target), 3)
    return true
end

--- Checks if a KV-FS entry exists and reads its contents.
--- Returns: { intro_start, outro_start, intro_duration, outro_duration } or nil
local function read_kvfs_entry(filename)
    if not config.cache_dir then return nil end
    local hash = fnv1a_64_hex(filename)
    local path = config.cache_dir .. "/" .. hash

    -- Use mp.utils.file_info for existence check (avoids pcall/io.open race)
    local info = utils.file_info(path)
    if not info or not info.is_file then return nil end

    local f = io.open(path, "r")
    if not f then return nil end

    local content = f:read("*a")
    f:close()

    -- Parse TSV: intro_start \t outro_start \t intro_duration \t outro_duration
    local parts = {}
    for field in (content .. "\t"):gmatch("([^\t]*)\t") do
        parts[#parts + 1] = field
    end

    if #parts ~= 4 then
        msg.warn("KV-FS schema mismatch for hash " .. hash)
        return nil
    end

    local function parse_val(s)
        s = s:match("^%s*(.-)%s*$") -- trim
        if s == "-" or s == "" then return nil end
        return tonumber(s)
    end

    return {
        intro_start    = parse_val(parts[1]),
        outro_start    = parse_val(parts[2]),
        intro_duration = parse_val(parts[3]) or 0,
        outro_duration = parse_val(parts[4]) or 0,
    }
end

--- Called on file-loaded: always loads KV-FS entry.
local function on_file_loaded()
    current_entry = nil
    last_position = nil

    local filename = mp.get_property("filename")
    if not filename then return end

    local entry = read_kvfs_entry(filename)
    if not entry then
        msg.info("No KV-FS entry for: " .. filename)
        return
    end

    current_entry = entry
    msg.info("KV-FS entry loaded for: " .. filename)
end

--- Single global periodic observer. Runs every 0.5s, always.
--- Checks the current playback position against intro/outro ranges
--- and skips if the corresponding toggle is enabled.
mp.add_periodic_timer(0.5, function()
    if not current_entry then return end

    local pos = mp.get_property_number("time-pos")
    if not pos then return end

    -- Check intro
    if config.skip_intro and current_entry.intro_start then
        if in_range(pos, current_entry.intro_start, current_entry.intro_duration) then
            skip_segment("Intro", current_entry.intro_start, current_entry.intro_duration)
            return
        end
    end

    -- Check outro
    if config.skip_outro and current_entry.outro_start then
        if in_range(pos, current_entry.outro_start, current_entry.outro_duration) then
            skip_segment("Outro", current_entry.outro_start, current_entry.outro_duration)
            return
        end
    end
end)

--- Toggle skip intro.
mp.add_key_binding("Ctrl+i", "anatro-toggle-intro", function()
    config.skip_intro = not config.skip_intro
    local state = config.skip_intro and "ON" or "OFF"
    mp.osd_message("Anatro: Skip Intro → " .. state, 2)
end)

--- Toggle skip outro.
mp.add_key_binding("Ctrl+o", "anatro-toggle-outro", function()
    config.skip_outro = not config.skip_outro
    local state = config.skip_outro and "ON" or "OFF"
    mp.osd_message("Anatro: Skip Outro → " .. state, 2)
end)

--- Undo last skip: return to the position before the skip.
mp.add_key_binding("BS", "anatro-undo-skip", function()
    if last_position then
        mp.commandv("seek", tostring(last_position), "absolute", "exact")
        mp.osd_message("⏪ Returned to " .. format_time(last_position), 2)
        last_position = nil
    else
        mp.osd_message("Anatro: No skip to undo", 2)
    end
end)

-- Register main event handler
mp.register_event("file-loaded", on_file_loaded)
