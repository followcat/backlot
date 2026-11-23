//! Neutral cmd permission-prompt screens at three terminal widths
//! (S12a). The same prompt, re-wrapped by hand: the 21-column capture
//! from a real narrow terminal, a medium width and a wide one.

/// The command all three fixtures ask about.
pub(crate) const CMD_FULL: &str = "backlot --help 2>&1 | head -5; echo ===; \
backlot runner --help 2>&1 | head -20; echo ===; \
backlot setup --help 2>&1 | head -20";

/// The prompt as captured at 21 columns.
pub(crate) const CMD_NARROW: &str = "\
Execute Shell Command
Command Code needs to
 execute backlot
--help 2>&1 | head
-5; echo ===; backlot
 runner --help 2>&1 |
 head -20; echo ===;
backlot setup --help
2>&1 | head -20.
Press [ctrl+e] to
explain this command
❯ 1. Yes
  2. Yes, don't ask
 again for `backlot`
 commands in this
 project
 3. No, tell Command
Code what to do
differently
↑/↓ navigate · enter
select · ctrl+e
explain · Run cmd
--yolo to bypass all
permissions (Docs ↗)";

/// The same prompt re-wrapped around 60 columns.
pub(crate) const CMD_MEDIUM: &str = "\
Execute Shell Command
Command Code needs to execute backlot
--help 2>&1 | head -5; echo ===; backlot
runner --help 2>&1 | head -20; echo ===;
backlot setup --help 2>&1 | head -20.
Press [ctrl+e] to explain this command
❯ 1. Yes
  2. Yes, don't ask again for `backlot`
commands in this project
  3. No, tell Command Code what to do differently
↑/↓ navigate · enter select · ctrl+e explain
· Run cmd --yolo to bypass all permissions (Docs ↗)";

/// The same prompt on a wide terminal.
pub(crate) const CMD_WIDE: &str = "\
Execute Shell Command
Command Code needs to execute backlot --help 2>&1 | head -5; echo ===; backlot runner --help 2>&1 | head
-20; echo ===; backlot setup --help 2>&1 | head -20.
Press [ctrl+e] to explain this command
❯ 1. Yes
  2. Yes, don't ask again for `backlot` commands in this project
  3. No, tell Command Code what to do differently
↑/↓ navigate · enter select · ctrl+e explain · Run cmd --yolo to bypass all permissions (Docs ↗)";
