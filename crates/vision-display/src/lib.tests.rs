use super::*;

#[test]
fn reads_a_key_as_its_modifiers_and_its_keysym() {
    let chord = |modifiers: &[u32], key: u32| Chord {
        modifiers: modifiers.to_vec(),
        key,
    };
    for (text, expected) in [
        ("a", chord(&[], 0x61)),
        ("A", chord(&[], 0x41)),
        ("ctrl+shift+t", chord(&[0xffe3, 0xffe1], 0x74)),
        ("ctrl+b", chord(&[0xffe3], 0x62)),
        ("ctrl++", chord(&[0xffe3], 0x2b)),
        ("+", chord(&[], 0x2b)),
        ("Enter", chord(&[], 0xff0d)),
        ("alt+f12", chord(&[0xffe9], 0xffc9)),
        ("é", chord(&[], 0xe9)),
        ("→", chord(&[], 0x0100_2192)),
    ] {
        assert_eq!(Chord::parse(text).unwrap(), expected, "{text}");
    }
    for text in ["", "hyper+a", "f13", "enterr", "ctrl+"] {
        assert!(Chord::parse(text).is_err(), "{text}");
    }
}
