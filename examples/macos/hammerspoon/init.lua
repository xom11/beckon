-- beckon bindings for macOS via Hammerspoon.
--
-- Append this block to ~/.hammerspoon/init.lua, then reload Hammerspoon
-- (cmd+ctrl+R, or click the Hammerspoon menu bar icon → Reload Config).
--
-- Requires beckon installed. The `BECKON` path below assumes Cargo
-- (`cargo install --git https://github.com/xom11/beckon`) — run
-- `which beckon` to confirm and adjust if you installed it elsewhere.
--
-- IMPORTANT: macOS requires Accessibility permission to focus other
-- apps' windows. Grant in:
--   System Settings → Privacy & Security → Accessibility
-- Add the beckon binary (the absolute path BECKON points to). Without
-- it beckon can still launch and focus the frontmost window of an app,
-- but cycling between multiple windows of the same app degrades.

local hyper = { "cmd", "ctrl", "alt" }
local BECKON = os.getenv("HOME") .. "/.cargo/bin/beckon"

-- We use hs.task with the absolute path. DO NOT use `hs.execute(cmd, true)`
-- — the second arg `true` makes Hammerspoon source your login shell
-- (~/.zshrc) before each invocation, which can add hundreds of ms (or
-- several seconds on heavy zshrc setups) to every hotkey press, fully
-- swamping beckon's native ~50ms hot path.
local function beckon(name)
    hs.task.new(BECKON, function(exitCode, _, stderr)
        if exitCode ~= 0 then
            hs.alert.show("beckon " .. name .. ": " .. (stderr or ""), 3)
        end
    end, { name }):start()
end

hs.hotkey.bind(hyper, "space", function() beckon("kitty") end)
hs.hotkey.bind(hyper, "c",     function() beckon("Claude") end)
hs.hotkey.bind(hyper, "b",     function() beckon("Brave Browser") end)
hs.hotkey.bind(hyper, "e",     function() beckon("Cursor") end)
hs.hotkey.bind(hyper, "d",     function() beckon("Discord") end)
