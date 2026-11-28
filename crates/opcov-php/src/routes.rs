use std::fmt;

use serde::{Deserialize, Serialize};

const CLOSURE: &str = "Closure";
const INVOKE: &str = "__invoke";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Route {
    pub methods: Vec<String>,
    pub uri: String,
    pub name: Option<String>,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Action {
    pub(crate) class: String,
    pub(crate) method: String,
}

#[derive(Debug, Default, Clone)]
pub struct Routes {
    pub(crate) routes: Vec<(Route, Action)>,
}

#[derive(Debug, Deserialize)]
struct Listed {
    domain: Option<String>,
    method: String,
    uri: String,
    name: Option<String>,
    action: String,
}

#[derive(Debug)]
pub struct RoutesError(serde_json::Error);

impl fmt::Display for RoutesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "the routes are not the JSON array `php artisan route:list --json` writes: {}",
            self.0
        );
    }
}

impl std::error::Error for RoutesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        return Some(&self.0);
    }
}

fn action(listed: &str) -> Option<Action> {
    if listed == CLOSURE {
        return None;
    }
    let (class, method) = listed.split_once('@').unwrap_or((listed, INVOKE));
    return Some(Action {
        class: class.trim_start_matches('\\').to_owned(),
        method: method.to_owned(),
    });
}

impl Routes {
    pub fn parse(json: &[u8]) -> Result<Self, RoutesError> {
        let listed: Vec<Listed> = serde_json::from_slice(json).map_err(RoutesError)?;
        let routes = listed
            .into_iter()
            .filter_map(|listed| {
                let action = action(&listed.action)?;
                let route = Route {
                    methods: listed.method.split('|').map(str::to_owned).collect(),
                    uri: listed.uri,
                    name: listed.name,
                    domain: listed.domain,
                };
                return Some((route, action));
            })
            .collect();
        return Ok(Self { routes });
    }
}
