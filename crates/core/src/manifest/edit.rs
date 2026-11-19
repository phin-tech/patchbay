//! Changing the person's own `patchbay.toml` from the app, keeping its comments and the order of what's there.

use toml_edit::{DocumentMut, Item, Table};

use super::*;
use crate::paste::Pasted;

fn document(text: &str) -> Result<DocumentMut, ManifestError> {
    text.parse::<DocumentMut>()
        .map_err(|error| ManifestError(error.to_string()))
}

/// The table `key` at the top of `doc`, made if it isn't there, holding its entries as `[key.name]` tables.
fn section<'a>(doc: &'a mut DocumentMut, key: &str) -> Result<&'a mut Table, ManifestError> {
    doc.entry(key)
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_mut()
        .ok_or_else(|| ManifestError(format!("`{key}` isn't a table")))
}

/// `value` as a TOML table, its maps written inline (`env = { … }`) so each server is one block.
fn table_of(value: &impl Serialize) -> Result<Table, ManifestError> {
    let text = toml::to_string(value).map_err(|error| ManifestError(error.to_string()))?;
    let mut table = document(&text)?.as_table().clone();
    for key in ["env", "headers", "tools", "oauth"] {
        if let Some(Item::Table(inner)) = table.get(key) {
            let inline = inner.clone().into_inline_table();
            table.insert(key, Item::Value(toml_edit::Value::InlineTable(inline)));
        }
    }
    table.set_implicit(false);
    Ok(table)
}

/// The manifest `text` with `servers` in it, each replacing any of the same name, and the secrets they name
/// described if they aren't already.
pub fn add(text: &str, servers: &[Pasted]) -> Result<String, ManifestError> {
    // With nothing but comments in it, the comments would be read as trailing and end up under what's added; the new
    // tables go after them instead.
    if document(text)?.as_table().is_empty() && !text.trim().is_empty() {
        let added = add("", servers)?;
        return Ok(format!("{}\n\n{added}", text.trim_end()));
    }
    let mut doc = document(text)?;
    for server in servers {
        for secret in &server.secrets {
            let secrets = section(&mut doc, "secrets")?;
            if !secrets.contains_key(&secret.name) {
                secrets.insert(&secret.name, Item::Table(table_of(&secret.spec)?));
            }
        }
        let mut table = table_of(&server.spec)?;
        let servers = section(&mut doc, "servers")?;
        // A server replaced stays where it was, with the comment above it.
        if let Some(old) = servers.get(&server.name).and_then(Item::as_table) {
            *table.decor_mut() = old.decor().clone();
            table.set_position(old.position());
        }
        servers.insert(&server.name, Item::Table(table));
    }
    Ok(doc.to_string())
}

/// The manifest `text` without the server `name`.
pub fn remove(text: &str, name: &str) -> Result<String, ManifestError> {
    let mut doc = document(text)?;
    if let Some(servers) = doc.get_mut("servers").and_then(Item::as_table_mut) {
        servers.remove(name);
    }
    Ok(doc.to_string())
}

/// The manifest `text` turning the server `name` on or off: how a team's server is left out without changing the
/// team's manifest.
pub fn set_enabled(text: &str, name: &str, enabled: bool) -> Result<String, ManifestError> {
    let mut doc = document(text)?;
    let servers = section(&mut doc, "servers")?;
    let server = servers
        .entry(name)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| ManifestError(format!("`servers.{name}` isn't a table")))?;
    if enabled {
        server.remove("enabled");
    } else {
        server.insert("enabled", toml_edit::value(false));
    }
    // A table left with nothing in it said only that the server was off.
    if server.is_empty() {
        servers.remove(name);
    }
    Ok(doc.to_string())
}

/// The manifest `text` without what it said only to turn the servers `names` off: once a team that had them is gone,
/// such a table would be a server with nothing to run. One that says more is the person's own and stays.
pub fn forget_switches(text: &str, names: &[String]) -> Result<String, ManifestError> {
    let mut doc = document(text)?;
    if let Some(servers) = doc.get_mut("servers").and_then(Item::as_table_mut) {
        for name in names {
            let only_a_switch = servers
                .get(name)
                .and_then(Item::as_table)
                .is_some_and(|table| table.iter().all(|(key, _)| key == "enabled"));
            if only_a_switch {
                servers.remove(name);
            }
        }
    }
    Ok(doc.to_string())
}

/// The manifest `text` with the profile `name` gathering `servers`, in place of any it had.
pub fn set_profile(text: &str, name: &str, servers: &[String]) -> Result<String, ManifestError> {
    let mut doc = document(text)?;
    let table = table_of(&ProfileSpec {
        servers: servers.to_vec(),
    })?;
    section(&mut doc, "profiles")?.insert(name, Item::Table(table));
    Ok(doc.to_string())
}

/// The manifest `text` without the profile `name`.
pub fn remove_profile(text: &str, name: &str) -> Result<String, ManifestError> {
    let mut doc = document(text)?;
    if let Some(profiles) = doc.get_mut("profiles").and_then(Item::as_table_mut) {
        profiles.remove(name);
    }
    Ok(doc.to_string())
}

/// The server `name` from the manifest `text` as a TOML block of its own, with the secrets it names described, for
/// the person to change and paste back.
pub fn server_toml(text: &str, name: &str) -> Option<String> {
    let manifest = Manifest::from_toml(text).ok()?;
    let spec = manifest.servers.get(name)?.clone();
    let mut alone = Manifest::default();
    for (secret, described) in &manifest.secrets {
        let named = [spec.env.as_ref(), spec.headers.as_ref()]
            .into_iter()
            .flatten()
            .flat_map(BTreeMap::values)
            .chain(spec.args.iter().flatten())
            .any(|value| {
                Template::parse(value)
                    .is_ok_and(|template| template.secrets().any(|used| used == secret))
            });
        if named {
            alone.secrets.insert(secret.clone(), described.clone());
        }
    }
    let pasted = Pasted {
        name: name.to_string(),
        spec,
        secrets: Vec::new(),
    };
    add(&secrets_only(&alone)?, &[pasted]).ok()
}

/// Just the `[secrets]` of `manifest`, as TOML.
fn secrets_only(manifest: &Manifest) -> Option<String> {
    let mut doc = DocumentMut::new();
    for (name, spec) in &manifest.secrets {
        section(&mut doc, "secrets")
            .ok()?
            .insert(name, Item::Table(table_of(spec).ok()?));
    }
    Some(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::{add, remove, remove_profile, server_toml, set_enabled, set_profile};
    use crate::paste::read;

    const MINE: &str = "# My servers.\n\n[servers.fetch]\ncommand = \"uvx\" # keep this\nargs = [\"mcp-server-fetch\"]\n";

    #[test]
    fn a_pasted_server_is_added_as_a_block_of_its_own_and_comments_stay() {
        let pasted = read(r#"{"brave": {"command": "npx", "args": ["-y", "brave-mcp"], "env": {"BRAVE_API_KEY": "abc"}}}"#)
            .unwrap();
        let text = add(MINE, &pasted).unwrap();
        insta::assert_snapshot!(text, @r#"
        # My servers.

        [servers.fetch]
        command = "uvx" # keep this
        args = ["mcp-server-fetch"]

        [servers.brave]
        command = "npx"
        args = ["-y", "brave-mcp"]
        env = { BRAVE_API_KEY = "secret:brave-api-key" }

        [secrets.brave-api-key]
        prompt = "brave's BRAVE_API_KEY"
        "#);
        assert!(crate::manifest::Manifest::from_toml(&text).is_ok());
    }

    #[test]
    fn a_server_comes_out_alone_and_goes_back_in_place_of_itself() {
        let pasted = read("https://mcp.linear.app/mcp").unwrap();
        let text = add(MINE, &pasted).unwrap();
        let alone = server_toml(&text, "linear").unwrap();
        insta::assert_snapshot!(alone, @r#"
        [servers.linear]
        url = "https://mcp.linear.app/mcp"
        auth = "oauth"
        "#);
        let changed = alone.replace("auth = \"oauth\"", "auth = \"none\"");
        let again = add(&remove(&text, "linear").unwrap(), &read(&changed).unwrap()).unwrap();
        assert!(again.contains("auth = \"none\""));
        assert_eq!(again.matches("[servers.linear]").count(), 1);
    }

    #[test]
    fn a_server_changed_in_place_keeps_its_place_and_its_comment() {
        let text = format!(
            "{MINE}\n# Linear's.\n[servers.linear]\nurl = \"https://mcp.linear.app/mcp\"\n"
        );
        let fetch =
            read("[servers.fetch]\ncommand = \"uvx\"\nargs = [\"mcp-server-fetch\", \"--raw\"]\n")
                .unwrap();
        let changed = add(&text, &fetch).unwrap();
        insta::assert_snapshot!(changed, @r#"
        # My servers.

        [servers.fetch]
        command = "uvx"
        args = ["mcp-server-fetch", "--raw"]

        # Linear's.
        [servers.linear]
        url = "https://mcp.linear.app/mcp"
        "#);
    }

    #[test]
    fn a_server_added_to_a_manifest_of_only_comments_goes_after_them() {
        let text = add("# Mine.\n", &read("https://mcp.linear.app/mcp").unwrap()).unwrap();
        assert!(text.starts_with("# Mine.\n\n[servers.linear]"), "{text}");
    }

    #[test]
    fn a_clients_own_profile_is_written_and_taken_out() {
        let text = set_profile(MINE, "cursor", &["fetch".to_string()]).unwrap();
        assert!(
            text.contains("[profiles.cursor]\nservers = [\"fetch\"]"),
            "{text}"
        );
        let again = set_profile(&text, "cursor", &[]).unwrap();
        assert!(again.contains("servers = []"), "{again}");
        assert!(!remove_profile(&again, "cursor").unwrap().contains("cursor"));
    }

    #[test]
    fn a_teams_server_is_turned_off_and_on_in_your_own_manifest() {
        let off = set_enabled(MINE, "github", false).unwrap();
        assert!(off.contains("[servers.github]\nenabled = false"), "{off}");
        let on = set_enabled(&off, "github", true).unwrap();
        assert!(!on.contains("github"), "{on}");
    }
}
