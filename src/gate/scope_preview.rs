use super::GateScope;
use crate::output::human::format_bytes;

/// Format a scope preview string for display before a destructive operation.
pub fn format_scope_preview(scope: &GateScope) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "About to {} {} object{} ({})\n",
        scope.action,
        scope.object_count,
        if scope.object_count == 1 { "" } else { "s" },
        format_bytes(scope.total_bytes),
    ));

    if scope.protected_count > 0 {
        out.push_str(&format!(
            "Protected paths affected: {}\n",
            scope.protected_count,
        ));
    }

    if !scope.examples.is_empty() {
        out.push_str("Examples:\n");
        for (i, example) in scope.examples.iter().enumerate() {
            if i >= 5 {
                out.push_str(&format!(
                    "  ... and {} more\n",
                    scope.examples.len() - 5
                ));
                break;
            }
            out.push_str(&format!("  {}\n", example));
        }
    }

    out.push_str("\nThis action is irreversible.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_scope_preview() {
        let scope = GateScope {
            action: "purge".to_string(),
            object_count: 482,
            total_bytes: 18_400_000_000,
            protected_count: 0,
            examples: vec![
                "/home/max/project/logs/app.log".to_string(),
                "/home/max/project/tmp/cache.db".to_string(),
            ],
        };
        let preview = format_scope_preview(&scope);
        assert!(preview.contains("482 objects"));
        assert!(preview.contains("17.1 GB"));
        assert!(preview.contains("irreversible"));
        assert!(preview.contains("/home/max/project/logs/app.log"));
    }

    #[test]
    fn single_object_no_plural() {
        let scope = GateScope {
            action: "delete".to_string(),
            object_count: 1,
            total_bytes: 1024,
            protected_count: 0,
            examples: vec![],
        };
        let preview = format_scope_preview(&scope);
        assert!(preview.contains("1 object "));
        assert!(!preview.contains("objects"));
    }

    #[test]
    fn protected_paths_shown() {
        let scope = GateScope {
            action: "purge".to_string(),
            object_count: 10,
            total_bytes: 0,
            protected_count: 3,
            examples: vec![],
        };
        let preview = format_scope_preview(&scope);
        assert!(preview.contains("Protected paths affected: 3"));
    }

    #[test]
    fn examples_truncated_at_five() {
        let scope = GateScope {
            action: "purge".to_string(),
            object_count: 8,
            total_bytes: 0,
            protected_count: 0,
            examples: (0..8).map(|i| format!("/path/{}", i)).collect(),
        };
        let preview = format_scope_preview(&scope);
        assert!(preview.contains("/path/0"));
        assert!(preview.contains("/path/4"));
        assert!(preview.contains("... and 3 more"));
        assert!(!preview.contains("/path/5"));
    }
}
