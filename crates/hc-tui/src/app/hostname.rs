//! OS hostname facts are distinct from core names and relay aliases.
use crate::chrome::snapshot::ClientEndpointId;
use hc_proto::hello::feature;
use hc_proto::watch::{Link, View};

pub(super) fn for_endpoint<'a>(view: &'a View, endpoint: &ClientEndpointId) -> Option<&'a str> {
    if view.inventory.is_offline() || !matches!(view.link, Link::Online | Link::Static { .. }) {
        return None;
    }
    let hostname = match endpoint {
        ClientEndpointId::Local => {
            if !view
                .features
                .iter()
                .any(|feature| feature == feature::OS_HOSTNAME)
            {
                return None;
            }
            view.core.as_ref()?.hostname.as_deref()?
        }
        ClientEndpointId::Remote(name) => {
            if view.rpc != Link::Online {
                return None;
            }
            let machine = view.machines.as_ref()?.iter().find(|m| m.name == *name)?;
            if machine.state != "connected" {
                return None;
            }
            machine.hostname.as_deref()?
        }
    };
    (!hostname.is_empty()).then_some(hostname)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidebar::fixture::{machine, view};

    #[test]
    fn hostname_never_falls_back_to_configured_name_or_alias() {
        let mut v = view(vec![], vec![]);
        v.core.as_mut().unwrap().name = "configured-name".into();
        assert_eq!(for_endpoint(&v, &ClientEndpointId::Local), None);
        v.core.as_mut().unwrap().hostname = Some("host-a".into());
        assert_eq!(for_endpoint(&v, &ClientEndpointId::Local), None);
        v.features.push(feature::OS_HOSTNAME.into());
        assert_eq!(for_endpoint(&v, &ClientEndpointId::Local), Some("host-a"));
        v.link = Link::Offline {
            attempt: 1,
            error: "lost".into(),
        };
        assert_eq!(for_endpoint(&v, &ClientEndpointId::Local), None);
    }

    #[test]
    fn remote_hostname_requires_a_fresh_connected_machine_observation() {
        let mut remote = machine("alias-b", "connected");
        remote.host = "configured-remote".into();
        let id = ClientEndpointId::Remote("alias-b".into());
        let mut v = view(vec![], vec![remote]);
        assert_eq!(for_endpoint(&v, &id), None);
        v.machines.as_mut().unwrap()[0].hostname = Some("host-b".into());
        assert_eq!(for_endpoint(&v, &id), Some("host-b"));
        v.machines.as_mut().unwrap()[0].state = "unreachable".into();
        assert_eq!(for_endpoint(&v, &id), None);
        v.machines.as_mut().unwrap()[0].state = "connected".into();
        v.rpc = Link::Connecting { attempt: 2 };
        assert_eq!(for_endpoint(&v, &id), None);
    }
}
