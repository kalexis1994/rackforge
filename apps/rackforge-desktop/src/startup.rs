use crate::{paths, web};
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct Options {
    pub(crate) web_preferences: web::WebServerPreferences,
    pub(crate) rackforge_root: PathBuf,
    pub(crate) plugins_root: PathBuf,
    pub(crate) plugin_store_root: Option<PathBuf>,
    pub(crate) data_root: PathBuf,
    pub(crate) install_archives: Vec<PathBuf>,
}

#[derive(Debug)]
struct CliOptions {
    port: Option<u16>,
    lan: bool,
    rackforge_root: Option<PathBuf>,
    plugins_root: Option<PathBuf>,
    data_root: Option<PathBuf>,
    install_archives: Vec<PathBuf>,
}

pub(crate) enum Startup {
    Ready(Options),
    FirstStart {
        web_preferences: web::WebServerPreferences,
        default_root: PathBuf,
        executable_directory: PathBuf,
        install_archives: Vec<PathBuf>,
    },
}

pub(crate) fn parse_startup() -> Result<Startup> {
    resolve_options(parse_cli_options(std::env::args().skip(1))?)
}

fn parse_cli_options(arguments: impl IntoIterator<Item = String>) -> Result<CliOptions> {
    let mut options = CliOptions {
        port: None,
        lan: false,
        rackforge_root: None,
        plugins_root: None,
        data_root: None,
        install_archives: Vec::new(),
    };
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--port" => {
                let port = arguments
                    .next()
                    .context("--port requires a value")?
                    .parse()
                    .context("invalid --port")?;
                if port < 1024 {
                    bail!("--port must be in 1024..=65535");
                }
                options.port = Some(port);
            }
            "--lan" => options.lan = true,
            "--rackforge-root" => {
                options.rackforge_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .context("--rackforge-root requires a directory")?,
                ));
            }
            "--plugins-root" => {
                options.plugins_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .context("--plugins-root requires a directory")?,
                ));
            }
            "--data-root" => {
                options.data_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .context("--data-root requires a directory")?,
                ));
            }
            "--install-plugin" => {
                options.install_archives.push(PathBuf::from(
                    arguments
                        .next()
                        .context("--install-plugin requires an .rfplugin file")?,
                ));
            }
            _ => bail!("unknown argument: {argument}"),
        }
    }
    Ok(options)
}

fn resolve_options(cli: CliOptions) -> Result<Startup> {
    let default_root = paths::default_root()?;
    let executable_directory = paths::executable_directory()?;
    let uses_legacy_paths =
        cli.rackforge_root.is_none() && (cli.plugins_root.is_some() || cli.data_root.is_some());

    let layout = if let Some(root) = cli.rackforge_root.as_deref() {
        paths::DesktopPaths::initialize(root)?
    } else if uses_legacy_paths {
        paths::DesktopPaths::initialize(&default_root)?
    } else if let Some(choice) = paths::load_choice(&default_root, &executable_directory)? {
        paths::DesktopPaths::initialize(choice.root)?
    } else {
        let mut web_preferences = web::WebServerPreferences::default();
        if let Some(port) = cli.port {
            web_preferences.port = port;
        }
        if cli.lan {
            web_preferences.enabled = true;
        }
        return Ok(Startup::FirstStart {
            web_preferences,
            default_root,
            executable_directory,
            install_archives: cli.install_archives,
        });
    };

    let web_config_path = layout.root.join("config/web.toml");
    let mut web_preferences =
        web::WebServerPreferences::load(&web_config_path)?.unwrap_or_default();
    if let Some(port) = cli.port {
        web_preferences.port = port;
    }
    if cli.lan {
        web_preferences.enabled = true;
    }
    Ok(Startup::Ready(Options {
        web_preferences,
        rackforge_root: layout.root,
        plugins_root: cli.plugins_root.unwrap_or(layout.legacy_plugins_root),
        plugin_store_root: (!uses_legacy_paths).then_some(layout.plugin_store_root),
        data_root: cli.data_root.unwrap_or(layout.data_root),
        install_archives: cli.install_archives,
    }))
}

pub(crate) fn options_from_layout(
    web_preferences: web::WebServerPreferences,
    layout: paths::DesktopPaths,
    install_archives: Vec<PathBuf>,
) -> Options {
    Options {
        web_preferences,
        rackforge_root: layout.root,
        plugins_root: layout.legacy_plugins_root,
        plugin_store_root: Some(layout.plugin_store_root),
        data_root: layout.data_root,
        install_archives,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_root_and_repeatable_plugin_install_arguments() {
        let parsed = parse_cli_options([
            "--port".to_owned(),
            "9123".to_owned(),
            "--lan".to_owned(),
            "--rackforge-root".to_owned(),
            "D:\\RackForge".to_owned(),
            "--install-plugin".to_owned(),
            "one.rfplugin".to_owned(),
            "--install-plugin".to_owned(),
            "two.rfplugin".to_owned(),
        ])
        .unwrap();

        assert_eq!(parsed.port, Some(9123));
        assert!(parsed.lan);
        assert_eq!(parsed.rackforge_root, Some(PathBuf::from("D:\\RackForge")));
        assert_eq!(
            parsed.install_archives,
            vec![PathBuf::from("one.rfplugin"), PathBuf::from("two.rfplugin")]
        );
    }

    #[test]
    fn keeps_legacy_plugin_and_data_arguments() {
        let parsed = parse_cli_options([
            "--plugins-root".to_owned(),
            "legacy-plugins".to_owned(),
            "--data-root".to_owned(),
            "legacy-data".to_owned(),
        ])
        .unwrap();

        assert_eq!(parsed.plugins_root, Some(PathBuf::from("legacy-plugins")));
        assert_eq!(parsed.data_root, Some(PathBuf::from("legacy-data")));
        assert!(parsed.rackforge_root.is_none());
    }
}
