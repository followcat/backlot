//! Late host facts exercise the live pane and its program-owned colors.

use super::*;

#[tokio::test]
async fn unknown_pitch_uses_source_local_fallback_without_reporting_a_measurement() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let (mut core, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(
        params
            .display
            .as_ref()
            .is_none_or(|facts| facts.cell.is_none())
    );
    core.serve_attach(testcore::attach_result(
        CH,
        testcore::minimal_terminal(),
        true,
    ))
    .await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[16t").await;
    assert_eq!(
        read_kind(&mut core, CH, Kind::AutomaticInput).await.payload,
        b"\x1b[6;16;8t"
    );
    assert_eq!(pane.vt().unwrap().cell_px(), (8, 16));
}

#[tokio::test]
async fn late_host_colors_update_live_defaults_and_keep_program_overrides() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(
        &mut pane,
        &mut rx,
        &mut core,
        b"\x1b]11;#102030\x1b\\\x1b]4;1;#405060\x1b\\\x1b[41mA\x1b[0mB",
    )
    .await;
    let mut host = HostFacts::default();
    let host_fg = Rgb {
        r: 221,
        g: 238,
        b: 255,
    };
    let host_bg = Rgb {
        r: 40,
        g: 44,
        b: 52,
    };
    let host_red = Rgb {
        r: 170,
        g: 187,
        b: 204,
    };
    let host_green = Rgb {
        r: 17,
        g: 34,
        b: 51,
    };
    host.theme.foreground = Some(host_fg);
    host.theme.background = Some(host_bg);
    host.theme.palette[1] = Some(host_red);
    host.theme.palette[2] = Some(host_green);
    pane.set_host_facts(host);

    let baseline = pane.paint();
    assert_eq!(baseline.initial_fg, Some(host_fg));
    assert_eq!(baseline.initial_bg, Some(host_bg));
    assert_eq!(baseline.initial_palette.unwrap()[1], host_red);
    let frame = pane.vt().unwrap().frame(0).unwrap();
    assert_eq!(frame.colors.foreground, host_fg);
    assert_eq!(
        frame.colors.background,
        Rgb {
            r: 16,
            g: 32,
            b: 48
        }
    );
    assert_eq!(
        frame.colors.palette[1],
        Rgb {
            r: 64,
            g: 80,
            b: 96
        }
    );
    assert_eq!(frame.colors.palette[2], host_green);

    // Resets reveal the NEW host values; a program override must never
    // become a host default merely because a reply arrived late.
    paint(
        &mut pane,
        &mut rx,
        &mut core,
        b"\x1b]111\x1b\\\x1b]104;1\x1b\\",
    )
    .await;
    let frame = pane.vt().unwrap().frame(0).unwrap();
    assert_eq!(frame.colors.background, host_bg);
    assert_eq!(frame.colors.palette[1], host_red);
    assert_eq!(frame.colors.palette[2], host_green);
}

#[tokio::test]
async fn late_measured_pitch_updates_live_replies_and_sends_facts() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"ready").await;
    let host = HostFacts {
        cell_px: Some((10, 20)),
        ..HostFacts::default()
    };
    pane.set_host_facts(host);
    let facts = read_kind(&mut core, CH, Kind::Facts).await;
    let facts: serde_json::Value = serde_json::from_slice(&facts.payload).unwrap();
    assert_eq!(facts["cell"], json!([10, 20]));
    assert!(facts["fg"].is_null() && facts["bg"].is_null());
    paint(&mut pane, &mut rx, &mut core, b"\x1b[16t").await;
    let reply = read_kind(&mut core, CH, Kind::AutomaticInput).await;
    assert_eq!(reply.payload, b"\x1b[6;20;10t");
}

#[tokio::test]
async fn an_observer_applies_host_defaults_without_sending_facts_or_input() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Observe)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"ready").await;
    let _ = read_kind(&mut core, CH, Kind::Credit).await;
    let host = HostFacts {
        cell_px: Some((10, 20)),
        ..HostFacts::default()
    };
    pane.set_host_facts(host);
    assert!(
        timeout(Duration::from_millis(30), core.read_stream(CH))
            .await
            .is_err()
    );
}
