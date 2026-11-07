use popproto_core::{decode, encode, ClientMsg};
#[test]
fn decode_load_hello_bytes() {
    let name = "load-0";
    let mut b: Vec<u8> = vec![0, 0, 0, 0, 6, 0];
    b.extend_from_slice(&6u64.to_le_bytes());
    b.extend_from_slice(name.as_bytes());
    b.push(0);
    let msg = decode::<ClientMsg>(&b);
    match msg {
        Some(ClientMsg::Hello {
            protocol,
            name,
            token,
        }) => println!("OK proto={protocol} name={name} token={token:?}"),
        other => panic!("decoded into: {other:?}"),
    }
    // контроль: серверный encode
    let ref_msg = ClientMsg::Hello {
        protocol: 6,
        name: "load-0".into(),
        token: None,
    };
    println!("server encode: {:?}", encode(&ref_msg));
}
