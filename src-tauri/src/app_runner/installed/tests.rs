use super::{Grants, check_view};
use crate::app_runner::{Controller, GuestTarget, Origin};

#[test]
fn agent_launch_is_exact_workspace_exact_vm_and_one_use() {
    let mut grants = Grants::default();
    grants.set("a", Some("v1".into()));
    assert!(grants.take_for_start("b", "v1", Origin::Agent).is_err());
    assert!(grants.take_for_start("a", "v2", Origin::Agent).is_err());
    grants.take_for_start("a", "v1", Origin::Agent).unwrap();
    assert!(grants.take_for_start("a", "v1", Origin::Agent).is_err());
}

#[test]
fn revocation_replacement_and_user_launch_invalidate_old_permission() {
    let mut grants = Grants::default();
    grants.set("a", Some("v1".into()));
    grants.set("b", Some("v2".into()));
    assert_eq!(grants.get("a"), None);
    grants.set("a", None);
    assert_eq!(grants.get("b"), Some("v2"));
    grants.set("b", None);
    assert!(grants.take_for_start("b", "v2", Origin::Agent).is_err());
    grants.set("b", Some("v2".into()));
    grants.take_for_start("b", "v1", Origin::User).unwrap();
    assert_eq!(grants.get("b"), None);
}

#[test]
fn agent_sees_no_screen_while_agent_access_is_off() {
    assert!(check_view(GuestTarget::Vm, Controller::User).is_err());
    assert!(check_view(GuestTarget::Vm, Controller::Agent).is_ok());
    assert!(check_view(GuestTarget::Default, Controller::User).is_err());
    assert!(check_view(GuestTarget::Default, Controller::Agent).is_ok());
}
