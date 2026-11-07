use popproto_core::{decode, ClientMsg};
#[test]
fn decode_browser_create() {
    let b: Vec<u8> = vec![
        2, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 208, 164, 208, 184, 208, 189, 208, 176, 208, 187, 53,
        0, 0, 0, 0, 0, 10, 1, 1, 0, 0, 128, 63, 0,
    ];
    match decode::<ClientMsg>(&b) {
        Some(ClientMsg::CreateRoom {
            name,
            password: _,
            mode,
            settings,
        }) => {
            println!(
                "OK name={name} mode={mode:?} max={} custom={:?}",
                settings.max_players,
                settings.custom_rows.is_some()
            );
        }
        other => panic!("decoded: {other:?}"),
    }
}
