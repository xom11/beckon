-- e2e drive for beckon's macOS settings window: it clicks, types, and reads
-- back, from a shell that cannot draw anything itself.
--
-- WHY THIS SHAPE. Two capabilities live in different processes on a dev Mac,
-- and no single one has both:
--
--   * an SSH/agent shell is in the `Background` bootstrap namespace, so it
--     cannot put an AppKit window on screen at all;
--   * a GUI app that CAN draw usually has no Accessibility grant, so its
--     `CGEventPost` is a silent no-op.
--
-- The split is closed by using one process for each half:
--
--   * `sudo launchctl asuser $(id -u) <cmd>` runs `beckon serve` in **Aqua**
--     -- measured, `launchctl managername` prints `Aqua` there;
--   * **Hammerspoon** drives it. It is an ordinary GUI app that already holds
--     Accessibility, so `hs.eventtap` posts REAL events and
--     `hs.axuielement` reads another app's whole control tree.
--
-- `hs.axuielement` is the part worth knowing: it enumerates buttons, check
-- boxes, popups and the tray menu of a different process, reads their titles
-- and values, and presses them. The AppleScript/System Events route does NOT
-- work for this -- it reported 0 windows for every app, including as a
-- control -- so a session that tries that one first concludes AX is a dead
-- end and hand-writes instructions for a human instead.
--
-- RUNNING IT:
--
--   ssh <mac> 'nohup sudo launchctl asuser $(id -u) \
--       ~/beckon-test/beckon serve ~/.config/beckon/apps.toml >/tmp/serve.log 2>&1 &'
--   scp testing/macos_settings_drive.lua <mac>:/tmp/drive.lua
--   ssh <mac> 'rm -f /tmp/drive.out; hs -c "dofile(\"/tmp/drive.lua\")" >/dev/null 2>&1;
--              for i in $(seq 1 25); do [ -f /tmp/drive.out ] && break; sleep 1; done;
--              cat /tmp/drive.out'
--
-- Results go to /tmp/drive.out rather than a return value, and that is not
-- style: `hs.timer.usleep` BLOCKS Hammerspoon's run loop, so `hs -c` gives up
-- with `receive timeout` while the script is still running fine. Reading the
-- file is how you get the answer; a timeout from `hs -c` is expected.
--
-- The binary needs Input Monitoring for the capture tests, and that grant is
-- per binary PATH -- so keep deploying to the same path rather than a fresh
-- one per build.
--
-- FOUR MORE TRAPS, each of which makes a working thing look broken. All were
-- hit, in this order, while measuring the save paths on 2026-08-17:
--
--   * `sudo launchctl asuser $(id -u) <cmd>` runs the child as **root**, so
--     beckon rewrites the config root-owned and the harness's own external
--     edit fails with `permission denied` -- which reads exactly like "Save
--     deleted my line". Use `sudo launchctl asuser $(id -u) sudo -u $USER`.
--   * A heredoc nested inside `ssh` eats quotes and backslashes out of a
--     shell command, so `printf '\n"a" = "b"\n'` arrives as
--     `printf na = bn`. Write the Lua locally and `scp` it; and prefer Lua's
--     own `io.open(path, "a")` to shelling out at all.
--   * A segment's caption is **`AXDescription`, not `AXTitle`**: an
--     `AXRadioButton` with `AXSubrole = AXSegment` answers nil for AXTitle
--     and `"Shortcuts  1"` for AXDescription. Reading the wrong attribute
--     makes a warning that IS on screen look absent.
--   * `settings_saw_external_change` sends a CLEAN model to a silent reload
--     and only a DIRTY one to the banner. A measurement that does not edit
--     something first is testing the other branch and will conclude the
--     watcher is dead when it is working perfectly.

local R = {}

local function say(s)
  R[#R + 1] = s
end

local function done()
  local f = io.open("/tmp/drive.out", "w")
  f:write(table.concat(R, "\n"))
  f:write("\n")
  f:close()
  return "written"
end

local function ok(name, cond, saw)
  local mark = "FAIL  "
  if cond then mark = "PASS  " end
  say(mark .. name .. "   saw: " .. tostring(saw))
end

local function app()
  for _, a in ipairs(hs.application.runningApplications()) do
    if a:name() == "beckon" and a:pid() > 50000 then return a end
  end
  return nil
end

local a = app()
if not a then
  say("FAIL  serve not running")
  return done()
end
local ax = hs.axuielement.applicationElement(a)

local function win()
  local ws = ax:attributeValue("AXWindows") or {}
  return ws[1]
end

local function controls()
  local found = { button = {}, check = {}, popup = {}, radio = {}, tbl = {} }
  local function walk(el, d)
    if d > 6 then return end
    for _, c in ipairs(el:attributeValue("AXChildren") or {}) do
      local role = tostring(c:attributeValue("AXRole"))
      local t = tostring(c:attributeValue("AXTitle"))
      if role == "AXButton" then
        found.button[#found.button + 1] = { el = c, title = t }
      elseif role == "AXCheckBox" then
        found.check[#found.check + 1] = { el = c, title = t }
      elseif role == "AXPopUpButton" then
        found.popup[#found.popup + 1] = { el = c }
      elseif role == "AXRadioButton" then
        found.radio[#found.radio + 1] = { el = c }
      elseif role == "AXTable" or role == "AXOutline" then
        found.tbl[#found.tbl + 1] = { el = c }
      end
      walk(c, d + 1)
    end
  end
  local w = win()
  if w then walk(w, 0) end
  return found
end

local function button(title)
  for _, b in ipairs(controls().button) do
    if b.title == title then return b.el end
  end
  return nil
end

local w = win()
if not w then
  say("FAIL  settings window not open")
  return done()
end
say("window: " .. tostring(w:attributeValue("AXTitle")))

local c0 = controls()
if #c0.tbl > 0 then
  local tb = c0.tbl[1].el
  local rows = tb:attributeValue("AXRows")
  if not rows then rows = tb:attributeValue("AXChildren") end
  rows = rows or {}
  if #rows > 0 then
    rows[1]:setAttributeValue("AXSelected", true)
    hs.timer.usleep(400000)
    say("selected row 1 of " .. #rows)
  else
    say("note: table has no rows")
  end
else
  say("note: no AXTable found")
end

local rec = button("Record")
if not rec then
  say("FAIL  no Record button")
  return done()
end
rec:performAction("AXPress")
hs.timer.usleep(700000)
local armed = button("Stop")
local sawT1 = "still Record"
if armed then sawT1 = "Stop" end
ok("T1 Record arms and reads Stop", armed ~= nil, sawT1)

hs.eventtap.keyStroke({ "ctrl", "cmd", "alt" }, "b", 20000)
hs.timer.usleep(1200000)

local after = controls()
local nticked = 0
local names = {}
for _, cb in ipairs(after.check) do
  local v = cb.el:attributeValue("AXValue")
  names[#names + 1] = cb.title .. "=" .. tostring(v)
  if v == 1 then nticked = nticked + 1 end
end
local key = "?"
if #after.popup > 0 then key = tostring(after.popup[1].el:attributeValue("AXValue")) end
local back = button("Record") ~= nil

ok("T2a chord ticks three boxes", nticked == 3, table.concat(names, " "))
ok("T2b key list shows b", string.lower(key) == "b", key)
ok("T2c recording ended by itself", back, tostring(back))

local rec2 = button("Record")
if rec2 then
  rec2:performAction("AXPress")
  hs.timer.usleep(600000)
  local a3 = button("Stop") ~= nil
  hs.eventtap.keyStroke({}, "escape", 20000)
  hs.timer.usleep(800000)
  local saw3 = "never armed"
  if a3 then saw3 = "armed, then Record=" .. tostring(button("Record") ~= nil) end
  ok("T3 bare Escape cancels", a3 and button("Record") ~= nil, saw3)
end

local rec3 = button("Record")
local rads = controls().radio
if rec3 and #rads >= 3 then
  rec3:performAction("AXPress")
  hs.timer.usleep(600000)
  local a4 = button("Stop") ~= nil
  controls().radio[3].el:performAction("AXPress")
  hs.timer.usleep(900000)
  controls().radio[1].el:performAction("AXPress")
  hs.timer.usleep(900000)
  local saw4 = "never armed"
  if a4 then saw4 = "armed, after switch Record=" .. tostring(button("Record") ~= nil) end
  ok("T4 page switch stops recording", a4 and button("Record") ~= nil, saw4)
end

return done()
