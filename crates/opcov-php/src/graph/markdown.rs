use std::collections::BTreeMap;
use std::fmt::Display;
use std::io::{self, Write};

use super::entries::EntryPoint;
use super::review::{Link, NodeCoverage, Ratio};
use super::{Graph, Location, Node, NodeChange, NodeId};
use crate::target::Resolution;

const SHOWN: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Links {
    pub template: String,
    pub base: String,
    pub head: String,
}

fn encoded(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            for digit in [byte >> 4, byte & 0x0f] {
                encoded.extend(
                    char::from_digit(u32::from(digit), 16)
                        .map(|hex| return hex.to_ascii_uppercase()),
                );
            }
        }
    }
    return encoded;
}

impl Links {
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "the template's placeholders look like format arguments but are filled in by replace"
    )]
    fn url(&self, commit: &str, location: &Location) -> String {
        return self
            .template
            .replace("{commit}", commit)
            .replace("{path}", &encoded(&location.path))
            .replace("{first}", &location.first.to_string())
            .replace("{last}", &location.last.to_string());
    }

    fn of(&self, node: &Node) -> Option<String> {
        if let Some(head) = &node.head {
            return Some(self.url(&self.head, head));
        }
        return node
            .base
            .as_ref()
            .map(|base| return self.url(&self.base, base));
    }
}

fn code(text: impl Display) -> String {
    return format!("`{text}`");
}

fn cell(text: &str) -> String {
    return text.replace('|', "\\|");
}

const fn change_name(change: NodeChange) -> &'static str {
    return match change {
        NodeChange::Added => "added",
        NodeChange::Removed => "removed",
        NodeChange::SignatureChanged => "signature changed",
        NodeChange::BodyChanged => "body changed",
        NodeChange::FormattingChanged => "formatting only",
        NodeChange::Unchanged => "unchanged",
    };
}

fn ratio(ratio: Ratio) -> String {
    return format!("{}/{}", ratio.covered, ratio.total);
}

fn coverage(coverage: Option<&NodeCoverage>) -> (String, String) {
    return match coverage {
        None => ("-".to_owned(), "-".to_owned()),
        Some(NodeCoverage::Measured {
            lines,
            outcomes,
            tests,
        }) => (
            format!("lines {}, branches {}", ratio(*lines), ratio(*outcomes)),
            tests.len().to_string(),
        ),
        Some(NodeCoverage::NeverLoaded) => ("never loaded".to_owned(), "0".to_owned()),
        Some(NodeCoverage::Unmeasured { reason }) => (cell(reason), "-".to_owned()),
    };
}

fn entry(entry: &EntryPoint) -> String {
    return match entry {
        EntryPoint::Route(route) => {
            format!("route {} {}", route.methods.join(", "), code(&route.uri))
        }
        EntryPoint::Command { name } => format!("command {}", code(name)),
        EntryPoint::Job => "queued job".to_owned(),
        EntryPoint::Listener { events } => format!(
            "listener of {}",
            events.iter().map(code).collect::<Vec<_>>().join(", ")
        ),
        EntryPoint::ModelHook { model, .. } => format!("model hook of {}", code(model)),
        EntryPoint::PhpunitTest { classes } => format!(
            "PHPUnit test in {}",
            classes.iter().map(code).collect::<Vec<_>>().join(", ")
        ),
        EntryPoint::PestTest => "Pest test file".to_owned(),
    };
}

fn namespace(id: &NodeId) -> String {
    return match id {
        NodeId::Member(target) => {
            let owner = target.class.as_deref().unwrap_or(&target.member);
            owner.rsplit_once('\\').map_or_else(
                || return "(global)".to_owned(),
                |(space, _)| return space.to_owned(),
            )
        }
        NodeId::Script(path) => path.rsplit_once('/').map_or_else(
            || return ".".to_owned(),
            |(directory, _)| return directory.to_owned(),
        ),
    };
}

fn list<T>(
    out: &mut dyn Write,
    level: &str,
    title: &str,
    explained: &str,
    items: &[T],
    line: impl Fn(&T) -> String,
) -> io::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    writeln!(out, "{level} {title} ({})\n\n{explained}\n", items.len())?;
    for item in items.iter().take(SHOWN) {
        writeln!(out, "- {}", line(item))?;
    }
    let more = items.len().saturating_sub(SHOWN);
    if more > 0 {
        writeln!(out, "- and {more} more")?;
    }
    return writeln!(out);
}

struct Renderer<'g> {
    links: Option<&'g Links>,
    nodes: BTreeMap<&'g NodeId, &'g Node>,
}

impl Renderer<'_> {
    fn member(&self, id: &NodeId) -> String {
        let shown = code(id);
        let url = self
            .links
            .zip(self.nodes.get(id))
            .and_then(|(links, node)| return links.of(node));
        return url.map_or_else(
            || return shown.clone(),
            |url| return format!("[{shown}]({url})"),
        );
    }

    fn link(&self, link: &Link) -> String {
        let via = link
            .via
            .as_ref()
            .map_or_else(String::new, |via| return format!(" via {}", code(via)));
        return format!(
            "{} → {}{via}",
            self.member(&link.from),
            self.member(&link.to)
        );
    }
}

impl Graph {
    fn changed(&self) -> Vec<&Node> {
        let mut changed: Vec<&Node> = self
            .nodes
            .iter()
            .filter(|node| return node.depth == 0)
            .collect();
        changed.sort_by_key(|node| {
            let impact = node.impact.map_or((0, 0, 0), |impact| {
                return (impact.entry_points, impact.proven_callers, impact.callers);
            });
            return (std::cmp::Reverse(impact), node.id.clone());
        });
        return changed;
    }

    fn headline(&self, out: &mut dyn Write) -> io::Result<()> {
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        for node in self.nodes.iter().filter(|node| return node.depth == 0) {
            let slot = counts.entry(change_name(node.change)).or_default();
            *slot = slot.saturating_add(1);
        }
        let counted: Vec<String> = counts
            .iter()
            .map(|(change, count)| return format!("{count} {change}"))
            .collect();
        writeln!(out, "## Change review\n")?;
        writeln!(
            out,
            "{} changed members ({}); {} unchanged members and scripts use them within depth {}.\n",
            counts.values().sum::<usize>(),
            counted.join(", "),
            self.nodes
                .iter()
                .filter(|node| return node.change == NodeChange::Unchanged)
                .count(),
            self.depth
        )?;
        return Ok(());
    }

    fn risk_count(&self) -> usize {
        let risks = &self.risks;
        return [
            risks.stops_resolving.len(),
            risks.starts_resolving.len(),
            risks.untouched_signature_callers.len(),
            risks.unproven_uses.len(),
            risks.broken_routes.len(),
            risks.untested.as_ref().map_or(0, Vec::len),
        ]
        .into_iter()
        .sum();
    }

    fn risk_sections(&self, out: &mut dyn Write, renderer: &Renderer<'_>) -> io::Result<()> {
        let risks = &self.risks;
        writeln!(out, "### Risks\n")?;
        if self.risk_count() == 0 {
            return writeln!(out, "None found.\n");
        }
        list(
            out,
            "####",
            "Untouched code that stops reaching a member",
            "These callers are outside the diff, and their call resolved on the base but not on the head.",
            &risks.stops_resolving,
            |found| return renderer.link(found),
        )?;
        list(
            out,
            "####",
            "Untouched code that now reaches a member",
            "These callers are outside the diff, and their call resolves to this member only on the head.",
            &risks.starts_resolving,
            |found| return renderer.link(found),
        )?;
        list(
            out,
            "####",
            "Untouched callers of changed signatures",
            "These callers are outside the diff and call a member whose declaration changed.",
            &risks.untouched_signature_callers,
            |found| return renderer.link(found),
        )?;
        list(
            out,
            "####",
            "Uses matched by name only",
            "The receiver's type is unknown, so these uses of changed members are not proven.",
            &risks.unproven_uses,
            |found| return renderer.link(found),
        )?;
        list(
            out,
            "####",
            "Routes whose method is gone",
            "The route table lists these routes, but the head has no method for their action.",
            &risks.broken_routes,
            |broken| {
                return format!(
                    "{} {} → {}",
                    broken.route.methods.join(", "),
                    code(&broken.route.uri),
                    code(&broken.action)
                );
            },
        )?;
        if let Some(untested) = &risks.untested {
            list(
                out,
                "####",
                "Members no baseline test reached",
                "The baseline report records no test that ran these members on the base.",
                untested,
                |id| return renderer.member(id),
            )?;
        }
        return Ok(());
    }

    fn changed_table(&self, out: &mut dyn Write, renderer: &Renderer<'_>) -> io::Result<()> {
        let changed = self.changed();
        let measured = self.risks.untested.is_some();
        writeln!(out, "### Changed members, by impact\n")?;
        if measured {
            writeln!(
                out,
                "| Member | Change | Callers (proven) | Entry points | Coverage at base | Tests |"
            )?;
            writeln!(out, "| --- | --- | --- | --- | --- | --- |")?;
        } else {
            writeln!(out, "| Member | Change | Callers (proven) | Entry points |")?;
            writeln!(out, "| --- | --- | --- | --- |")?;
        }
        for node in changed.iter().take(SHOWN) {
            let (callers, entry_points) = node.impact.map_or_else(
                || return ("-".to_owned(), "-".to_owned()),
                |impact| {
                    return (
                        format!("{} ({})", impact.callers, impact.proven_callers),
                        impact.entry_points.to_string(),
                    );
                },
            );
            let measurement = if measured {
                let (covered, tests) = coverage(node.coverage.as_ref());
                format!(" {covered} | {tests} |")
            } else {
                String::new()
            };
            writeln!(
                out,
                "| {} | {} | {callers} | {entry_points} |{measurement}",
                cell(&renderer.member(&node.id)),
                change_name(node.change),
            )?;
        }
        let more = changed.len().saturating_sub(SHOWN);
        if more > 0 {
            writeln!(out, "\nand {more} more changed members.")?;
        }
        return writeln!(out);
    }

    fn entry_sections(&self, out: &mut dyn Write, renderer: &Renderer<'_>) -> io::Result<()> {
        let entered: Vec<(&Node, &EntryPoint)> = self
            .nodes
            .iter()
            .flat_map(|node| return node.entries.iter().map(move |entry| return (node, entry)))
            .collect();
        list(
            out,
            "###",
            "Entry points the change reaches",
            "The framework or a test runner enters these members; depth 0 is changed code.",
            &entered,
            |(node, point)| {
                let proven = if node.resolution == Resolution::Typed {
                    ""
                } else {
                    ", by name"
                };
                return format!(
                    "{} at {} (depth {}{proven})",
                    entry(point),
                    renderer.member(&node.id),
                    node.depth
                );
            },
        )?;
        return Ok(());
    }

    fn namespace_section(&self, out: &mut dyn Write) -> io::Result<()> {
        let mut spaces: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for node in self
            .nodes
            .iter()
            .filter(|node| return node.change == NodeChange::Unchanged)
        {
            let (all, proven) = spaces.entry(namespace(&node.id)).or_default();
            *all = all.saturating_add(1);
            if node.resolution == Resolution::Typed {
                *proven = proven.saturating_add(1);
            }
        }
        let mut folded: Vec<(String, (usize, usize))> = spaces.into_iter().collect();
        folded.sort_by_key(|(space, (all, _))| return (std::cmp::Reverse(*all), space.clone()));
        return list(
            out,
            "###",
            "Unchanged callers by namespace",
            "The unchanged members and scripts that use the change, per namespace, with how many a chain of typed calls connects to it.",
            &folded,
            |(space, (all, proven))| return format!("{}: {all} ({proven} proven)", code(space)),
        );
    }

    pub fn render_markdown(&self, out: &mut dyn Write, links: Option<&Links>) -> io::Result<()> {
        let renderer = Renderer {
            links,
            nodes: self
                .nodes
                .iter()
                .map(|node| return (&node.id, node))
                .collect(),
        };
        self.headline(out)?;
        self.risk_sections(out, &renderer)?;
        self.changed_table(out, &renderer)?;
        self.entry_sections(out, &renderer)?;
        return self.namespace_section(out);
    }
}
