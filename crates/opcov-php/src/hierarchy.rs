use std::collections::BTreeMap;

use mago_codex::metadata::CodebaseMetadata;

pub(crate) type TraitUsers = BTreeMap<String, Vec<String>>;

pub(crate) fn trait_users(codebase: &CodebaseMetadata) -> TraitUsers {
    let mut users = TraitUsers::new();
    let mut classes: Vec<_> = codebase.class_likes.values().collect();
    classes.sort_by_key(|class| return class.name);
    for class in classes {
        let mut used: Vec<String> = class
            .used_traits
            .iter()
            .map(|used| return used.to_string().to_ascii_lowercase())
            .collect();
        used.sort();
        for trait_name in used {
            users
                .entry(trait_name)
                .or_default()
                .push(class.original_name.to_string());
        }
    }
    return users;
}

#[derive(Clone, Copy)]
pub(crate) struct Hierarchy<'c>(pub(crate) &'c CodebaseMetadata);

impl<'c> Hierarchy<'c> {
    pub(crate) const fn codebase(self) -> &'c CodebaseMetadata {
        return self.0;
    }

    pub(crate) fn exists(self, class: &str) -> bool {
        return self.0.class_like_exists(class.as_bytes());
    }

    pub(crate) fn is(self, child: &str, parent: &str) -> bool {
        return child.eq_ignore_ascii_case(parent)
            || self.0.is_instance_of(child.as_bytes(), parent.as_bytes());
    }

    pub(crate) fn is_trait(self, class: &str) -> bool {
        return self.0.trait_exists(class.as_bytes());
    }

    pub(crate) fn uses_trait(self, class: &str, used: &str) -> bool {
        if self.0.class_uses_trait(class.as_bytes(), used.as_bytes()) {
            return true;
        }
        return self
            .0
            .get_class_like(class.as_bytes())
            .is_some_and(|metadata| {
                return metadata.all_parent_classes.iter().any(|parent| {
                    return self.0.class_uses_trait(parent.as_bytes(), used.as_bytes());
                });
            });
    }
}
