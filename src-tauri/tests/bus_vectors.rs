//! Les cas de conformité du bus et de suite.json, communs aux trois langages de la suite (cdx-ui bus/bus-vectors.json, recopié par charte-sync) :
//! le client Rust (cdx_bus.rs) doit donner exactement les réponses attendues, comme le client C++ (Audio Linker) et C# (cdx-windows).
use cdx_messages_lib::cdx_bus;
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("bus-vectors.json")).expect("bus-vectors.json lisible")
}

#[test]
fn the_line_limit_is_the_common_one() {
    assert_eq!(cdx_bus::MAX_LINE as u64, vectors()["max_line"].as_u64().unwrap());
}

#[test]
fn the_program_list_is_the_common_one() {
    let v = vectors();
    let want: Vec<&str> = v["apps"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
    let mut a: Vec<&str> = cdx_bus::APPS.to_vec();
    let mut b = want.clone();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    for name in want {
        assert!(cdx_bus::is_app(name), "{name}");
    }
}

#[test]
fn every_accent_case_gives_the_expected_answer() {
    let v = vectors();
    let mut bad = Vec::new();
    for c in v["accent"].as_array().unwrap() {
        let (json, fallback, expect) = (c["json"].as_str().unwrap(), c["fallback"].as_str().unwrap(), c["expect"].as_str().unwrap());
        let got = cdx_bus::parse_accent(json, fallback);
        if got != expect {
            bad.push(format!("{} : {json} → {got}, attendu {expect}", c["why"].as_str().unwrap_or("")));
        }
    }
    assert!(bad.is_empty(), "cas en écart :\n{}", bad.join("\n"));
}
