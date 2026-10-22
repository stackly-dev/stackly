//! The path of a Unix socket as `InstanceSummary` shows it (spec 0020,
//! *Socket Unix dengan token port*).
//!
//! The registry keeps a socket name only to fill a required column and to
//! tell two sockets apart; the plan renders the real one from the pinned
//! package with the effective ports of the instance. So the path shown is
//! the run folder plus the `socketName` template of the pinned package,
//! rendered with the effective port of each endpoint the template names,
//! and `null` while one of those ports is not known. The templates are read
//! from the pinned folder once per provider revision and kept in memory.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, PoisonError};

use stackly_core::PreparedLayout;
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{Endpoint, Instance, ProviderRevisionKey};
use stackly_platform::Platform;

/// The `socketName` templates of every pinned provider revision this run
/// has read: endpoint name to template, as the manifest writes it.
pub struct SocketPaths {
    platform: Platform,
    prepared: PreparedLayout,
    templates: Mutex<HashMap<ProviderRevisionKey, BTreeMap<String, String>>>,
}

impl std::fmt::Debug for SocketPaths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocketPaths").finish_non_exhaustive()
    }
}

impl SocketPaths {
    /// A reader of the pinned packages under the folders of `prepared`.
    #[must_use]
    pub fn new(platform: Platform, prepared: PreparedLayout) -> Self {
        Self {
            platform,
            prepared,
            templates: Mutex::new(HashMap::new()),
        }
    }

    /// The socket path of each Unix endpoint of `instance` whose path is
    /// known now, by endpoint name.
    #[must_use]
    pub fn of(&self, instance: &Instance) -> BTreeMap<String, String> {
        if !instance
            .endpoints()
            .iter()
            .any(|endpoint| matches!(endpoint, Endpoint::Unix(_)))
        {
            return BTreeMap::new();
        }
        let templates = self.templates_of(instance.provider_revision());
        let run = self.prepared.layout().instance_paths(instance.id()).run;
        let port_of = |name: &str| {
            instance
                .endpoints()
                .iter()
                .find_map(|endpoint| match endpoint {
                    Endpoint::Ip(ip) if ip.name().as_str() == name => {
                        ip.effective_port().map(|port| port.get())
                    }
                    _ => None,
                })
        };
        templates
            .into_iter()
            .filter_map(|(endpoint, template)| {
                let file = render(&template, &port_of)?;
                Some((endpoint, run.join(file).to_string_lossy().into_owned()))
            })
            .collect()
    }

    fn templates_of(&self, key: &ProviderRevisionKey) -> BTreeMap<String, String> {
        if let Some(found) = self
            .templates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
        {
            return found.clone();
        }
        let dir = self.prepared.layout().provider_revision_dir(key);
        let package = read_package(self.platform.fs(), &dir)
            .ok()
            .and_then(|files| ValidatedPackage::from_files(files, NameCheck::Skip).ok());
        let Some(package) = package else {
            // A package that cannot be read now may be read later: nothing
            // is cached, and no path is shown.
            return BTreeMap::new();
        };
        let templates: BTreeMap<String, String> = package
            .endpoints()
            .filter_map(|spec| Some((spec.name().to_string(), spec.socket_name_template()?)))
            .collect();
        self.templates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), templates.clone());
        templates
    }
}

/// `template` with each `${endpoint.<name>.port}` replaced by that port, or
/// `None` while a port it names is not known. The manifest allows no other
/// token in a socket name.
fn render(template: &str, port_of: &dyn Fn(&str) -> Option<u16>) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find('}')?;
        let token = &after[..end];
        let name = token.strip_prefix("endpoint.")?.strip_suffix(".port")?;
        out.push_str(&port_of(name)?.to_string());
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_without_a_token_is_its_own_name() {
        assert_eq!(
            render("mysqld.sock", &|_| None),
            Some("mysqld.sock".to_owned())
        );
    }

    #[test]
    fn a_port_token_takes_the_port_of_its_endpoint() {
        let port = |name: &str| (name == "client").then_some(5432);
        assert_eq!(
            render(".s.PGSQL.${endpoint.client.port}", &port),
            Some(".s.PGSQL.5432".to_owned())
        );
    }

    #[test]
    fn a_port_that_is_not_known_yet_leaves_no_path() {
        assert_eq!(render(".s.PGSQL.${endpoint.client.port}", &|_| None), None);
    }

    #[test]
    fn a_token_that_is_not_a_port_leaves_no_path() {
        assert_eq!(render("a${endpoint.client.socket}", &|_| Some(1)), None);
        assert_eq!(render("a${unclosed", &|_| Some(1)), None);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod pinned {
    use std::path::Path;

    use stackly_core::prepare_layout;
    use stackly_core::supervisor::fixture::pin_package;
    use stackly_domain::{Endpoint, IpEndpoint, PortRequest, Transport, UnixEndpoint};
    use stackly_platform_macos::MacosPlatform;
    use stackly_testkit::{InstanceBuilder, RuntimeBuilder, TestRoot};

    use super::*;

    fn instance(client: PortRequest, bound: Option<u16>) -> (TestRoot, SocketPaths, Instance) {
        let root = TestRoot::new().expect("a test root");
        let platform = MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS");
        let prepared = prepare_layout(&platform).expect("the layout prepares");
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/postgresql");
        let record = pin_package(&platform, prepared.layout(), &package).expect("the package pins");
        let key = record.revision.key().clone();
        let runtime = RuntimeBuilder::new().build();
        let tcp = IpEndpoint::new(
            "client".parse().expect("a name"),
            Transport::Tcp,
            "127.0.0.1".parse().expect("an address"),
            client,
            bound.map(|port| stackly_domain::Port::new(port).expect("a port")),
        );
        let unix = UnixEndpoint::new(
            "socket".parse().expect("a name"),
            ".s.PGSQL.auto".parse().expect("a socket name"),
        );
        let instance = InstanceBuilder::new(&runtime, &key)
            .endpoints(vec![Endpoint::Ip(tcp), Endpoint::Unix(unix)])
            .build();
        (root, SocketPaths::new(platform, prepared), instance)
    }

    #[test]
    fn the_path_is_the_run_folder_and_the_template_of_the_pinned_package_with_the_port() {
        let (_root, sockets, instance) = instance(
            PortRequest::Fixed(stackly_domain::Port::new(5432).expect("a port")),
            None,
        );

        let paths = sockets.of(&instance);

        let run = sockets.prepared.layout().instance_paths(instance.id()).run;
        assert_eq!(
            paths.get("socket").map(String::as_str),
            Some(run.join(".s.PGSQL.5432").to_string_lossy().as_ref())
        );
    }

    #[test]
    fn an_auto_port_that_is_not_bound_yet_leaves_the_path_null_and_a_bound_one_gives_it() {
        let (_root, sockets, unbound) = instance(PortRequest::Auto, None);
        assert!(sockets.of(&unbound).is_empty());

        let (_root, sockets, bound) = instance(PortRequest::Auto, Some(49_152));
        let run = sockets.prepared.layout().instance_paths(bound.id()).run;
        assert_eq!(
            sockets.of(&bound).get("socket").map(String::as_str),
            Some(run.join(".s.PGSQL.49152").to_string_lossy().as_ref())
        );
    }
}
