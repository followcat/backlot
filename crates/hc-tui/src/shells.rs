//! Studio shell facts (U2): the label scheme and the read-side
//! helpers the cast, the creation path and the role keys share.
//! Pure over the inventory — nothing is stored. A shell is the
//! user's own terminal that belongs to a studio: the stage never
//! touches it and `backlot.studio` + `backlot.shell` mark it.

/// The label tying a shell (or a role) to its studio.
pub(crate) const STUDIO_LABEL: &str = "backlot.studio";
/// The label marking a studio shell — `backlot.shell=<n>`.
pub(crate) const SHELL_LABEL: &str = "backlot.shell";

/// A shell terminal's name — `bl-<studio>-sh<n>` can never collide
/// with a role terminal's: the fixed roles give `bl-<studio>-{producer,
/// director,writer}` and actors `bl-<studio>-t<n>` (task ids are the
/// generated `t<n>`), none of which is `sh<n>`; the `bl-` prefix keeps
/// the name out of the user's namespace anyway.
pub(crate) fn shell_name(studio: &str, n: u32) -> String {
    format!("bl-{studio}-sh{n}")
}

/// A terminal's owning machine — the `host-b/` prefix, `None` here.
fn machine_of(address: &str) -> Option<&str> {
    address.split_once('/').map(|(machine, _)| machine)
}

/// The next `backlot.shell` number for `studio` on `machine` (`None`
/// is this core) — one past the highest in that machine's inventory.
pub(crate) fn next_shell_n(
    view: &hc_proto::watch::View,
    machine: Option<&str>,
    studio: &str,
) -> u32 {
    view.inventory
        .terminals()
        .values()
        .filter(|t| machine_of(&t.address) == machine)
        .filter(|t| t.labels.get(STUDIO_LABEL).is_some_and(|s| s == studio))
        .filter_map(|t| t.labels.get(SHELL_LABEL))
        .filter_map(|s| s.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

/// Every `address` of `studio`'s shells on `machine` (`None` is this
/// core), sorted by shell number — the cast's `shells` group and the
/// role keys read it.
pub(crate) fn studio_shells(
    view: &hc_proto::watch::View,
    machine: Option<&str>,
    studio: &str,
) -> Vec<(u32, String)> {
    let mut shells: Vec<(u32, String)> = view
        .inventory
        .terminals()
        .values()
        .filter(|t| machine_of(&t.address) == machine)
        .filter(|t| t.labels.get(STUDIO_LABEL).is_some_and(|s| s == studio))
        .filter_map(|t| {
            Some((
                t.labels.get(SHELL_LABEL)?.parse::<u32>().ok()?,
                t.address.clone(),
            ))
        })
        .collect();
    shells.sort();
    shells
}

/// The `herdr.space`/`herdr.tab` ids a `shell <n>` tab gets in
/// `studio`'s space — the layout's space id and the tab id nested
/// under it, the same shape `space_of` and `tab_key_of` derive from
/// the labels (`<space>/tab:<len>:<label>`).
pub(crate) fn shell_tab_id(space_label: &str, tab_label: &str) -> (String, String) {
    let space = format!("space:{}:{space_label}", space_label.len());
    (
        space.clone(),
        format!("{space}/tab:{}:{tab_label}", tab_label.len()),
    )
}
