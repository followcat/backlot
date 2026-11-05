//! Large semantic events must obey the pending-input byte budget.

use super::*;

#[tokio::test]
async fn oversized_key_metadata_is_rejected_with_visible_overflow_accounting() {
    for (text_bytes, raw_bytes) in [(300 * 1024, 0), (0, 300 * 1024), (150 * 1024, 150 * 1024)] {
        let (mut pane, mut rx, mut accept) = rig();
        pane.want(Some(spec(Role::Active)));
        let _core = serve(&mut accept, true).await;
        step(&mut pane, &mut rx).await; // No fresh paint: encoding must wait.
        let mut input = key('a');
        input.text_commit = (text_bytes > 0).then(|| "a".repeat(text_bytes));
        input.vt_bytes = vec![b'a'; raw_bytes];
        pane.key(&input);
        assert_eq!(
            pane.input_backlog(),
            (0, 1),
            "oversized event must be counted, not retained: text={text_bytes}, raw={raw_bytes}"
        );
    }
}
