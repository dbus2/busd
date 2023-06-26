use busd::config::{BusConfig, Element};
use std::error::Error;
use std::path::PathBuf;

#[test]
fn test_se() -> Result<(), Box<dyn Error>> {
    let mut elements = vec![];
    elements.push(Element::User("foo".into()));
    let c = BusConfig { elements };
    let _config = String::try_from(&c);
    Ok(())
}

#[test]
fn test_de() -> Result<(), Box<dyn Error>> {
    let input = r##"
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <user>foo</user>
  <listen>unix:path=/foo/bar</listen>
  <listen>unix:path=/foo/bar2</listen>
</busconfig>
"##;
    let _config = BusConfig::try_from(input)?;
    Ok(())
}

#[test]
fn valid_basic() -> Result<(), Box<dyn Error>> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/data/valid-config-files/basic.conf");
    let config = BusConfig::read(path)?;
    let n_listen = config
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Listen(_)))
        .count();
    assert_eq!(n_listen, 4);
    Ok(())
}

#[test]
fn valid_check_own_rules() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/valid-config-files/check-own-rules.conf");
    let _config = BusConfig::try_from(input)?;
    Ok(())
}

#[test]
fn valid_entities() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/valid-config-files/entities.conf");
    let _config = BusConfig::try_from(input)?;
    Ok(())
}

#[test]
fn valid_listen_unix_runtime() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/valid-config-files/listen-unix-runtime.conf");
    let _config = BusConfig::try_from(input)?;
    Ok(())
}

#[test]
fn valid_many_rules() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/valid-config-files/many-rules.conf");
    let _config = BusConfig::try_from(input)?;
    Ok(())
}

#[test]
fn valid_minimal() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/valid-config-files/minimal.conf");
    let _config = BusConfig::try_from(input)?;
    Ok(())
}

#[test]
fn valid_session() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/valid-config-files/session.conf");
    let _config = BusConfig::try_from(input)?;
    Ok(())
}

#[test]
fn valid_standard_session_dirs() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/valid-config-files/standard-session-dirs.conf");
    let _config = BusConfig::try_from(input)?;
    Ok(())
}

#[test]
fn invalid_apparmor_bad_attribute() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/apparmor-bad-attribute.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_bad_attribute() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/bad-attribute.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

// FIXME: how to #[serde(deny_unknown_fields)]
#[test]
#[ignore]
fn invalid_bad_attribute_2() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/bad-attribute-2.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_bad_element() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/bad-element.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_bad_limit() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/bad-limit.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_badselinux_1() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/badselinux-1.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_badselinux_2() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/badselinux-2.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_circular_1() -> Result<(), Box<dyn Error>> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/data/invalid-config-files/circular-1.conf");
    assert!(BusConfig::read(path).is_err());
    Ok(())
}

#[test]
fn invalid_circular_2() -> Result<(), Box<dyn Error>> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/data/invalid-config-files/circular-2.conf");
    assert!(BusConfig::read(path).is_err());
    Ok(())
}

#[test]
fn invalid_double_attribute() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/double-attribute.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

// FIXME: needs some semantic knowledge?
#[ignore]
#[test]
fn invalid_impossible_send() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/impossible-send.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_limit_no_name() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/limit-no-name.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_ludicrous_limit() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/ludicrous-limit.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_negative_limit() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/negative-limit.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_non_numeric_limit() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/non-numeric-limit.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_not_well_formed() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/not-well-formed.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_policy_bad_at_console() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-bad-at-console.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_policy_bad_attribute() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-bad-attribute.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_policy_bad_context() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-bad-context.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

// FIXME: how to #[serde(deny_unknown_fields)]
#[ignore]
#[test]
fn invalid_policy_bad_rule_attribute() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-bad-rule-attribute.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_policy_contradiction() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-contradiction.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

// FIXME: needs some semantic knowledge?
#[ignore]
#[test]
fn invalid_policy_member_no_path() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-member-no-path.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

// FIXME: needs some semantic knowledge?
#[ignore]
#[test]
fn invalid_policy_mixed() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-mixed.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_policy_no_attributes() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-no-attributes.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_policy_no_rule_attribute() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/policy-no-rule-attribute.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_send_and_receive() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/send-and-receive.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

#[test]
fn invalid_truncated_file() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/truncated-file.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}

// FIXME: needs some semantic knowledge?
#[ignore]
#[test]
fn invalid_unknown_limit() -> Result<(), Box<dyn Error>> {
    let input = include_str!("data/invalid-config-files/unknown-limit.conf");
    assert!(BusConfig::try_from(input).is_err());
    Ok(())
}
