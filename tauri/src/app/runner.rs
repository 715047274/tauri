#[allow(unused_imports)]
use std::{
  env,
  fs::{self, read_to_string},
  path::Path,
  process::Stdio,
  thread::spawn,
};

use webview_official::{SizeHint, Webview, WebviewBuilder};

use super::App;
#[cfg(embedded_server)]
use crate::api::tcp::{get_available_port, port_is_available};
use crate::app::AppContext;

#[allow(dead_code)]
enum Content<T> {
  Html(T),
  Url(T),
}

/// Main entry point for running the Webview
pub(crate) fn run(application: &mut App) -> crate::Result<()> {
  // setup the content using the config struct depending on the compile target
  let main_content = setup_content(&application.ctx)?;

  #[cfg(embedded_server)]
  {
    // setup the server url for the embedded-server
    let server_url = if let Content::Url(url) = &main_content {
      String::from(url)
    } else {
      String::from("")
    };

    // spawn the embedded server on our server url
    spawn_server(server_url, &application.ctx)?;
  }

  // build the webview
  let mut webview = build_webview(
    application,
    main_content,
    if application.splashscreen_html().is_some() {
      Some(Content::Html(
        application
          .splashscreen_html()
          .expect("failed to get splashscreen_html")
          .to_string(),
      ))
    } else {
      None
    },
  )?;

  crate::plugin::created(&mut webview);

  // spin up the updater process
  #[cfg(feature = "updater")]
  spawn_updater()?;

  // run the webview
  webview.run();

  Ok(())
}

// setup content for dev-server
#[cfg(dev)]
fn setup_content(app_config: &AppContext) -> crate::Result<Content<String>> {
  let config = &app_config.config;
  if config.build.dev_path.starts_with("http") {
    #[cfg(windows)]
    {
      let exempt_output = std::process::Command::new("CheckNetIsolation")
        .args(&vec!["LoopbackExempt", "-s"])
        .output()
        .expect("failed to read LoopbackExempt -s");

      if !exempt_output.status.success() {
        panic!("Failed to execute CheckNetIsolation LoopbackExempt -s");
      }

      let output_str = String::from_utf8_lossy(&exempt_output.stdout).to_lowercase();
      if !output_str.contains("win32webviewhost_cw5n1h2txyewy") {
        println!("Running Loopback command");
        runas::Command::new("powershell")
          .args(&[
            "CheckNetIsolation LoopbackExempt -a -n=\"Microsoft.Win32WebViewHost_cw5n1h2txyewy\"",
          ])
          .force_prompt(true)
          .status()
          .expect("failed to run Loopback command");
      }
    }
    Ok(Content::Url(config.build.dev_path.clone()))
  } else {
    let dev_dir = &config.build.dev_path;
    let dev_path = Path::new(dev_dir).join("index.tauri.html");
    if !dev_path.exists() {
      panic!(
        "Couldn't find 'index.tauri.html' inside {}; did you forget to run 'tauri dev'?",
        dev_dir
      );
    }
    Ok(Content::Html(format!(
      "data:text/html,{}",
      urlencoding::encode(&read_to_string(dev_path)?)
    )))
  }
}

// setup content for embedded server
#[cfg(embedded_server)]
fn setup_content(app_config: &AppContext) -> crate::Result<Content<String>> {
  let (port, valid) = setup_port(&app_config)?;
  let url = (if valid {
    setup_server_url(port, &app_config)
  } else {
    Err(anyhow::anyhow!("invalid port"))
  })
  .expect("Unable to setup URL");

  Ok(Content::Url(url))
}

// setup content for no-server
#[cfg(no_server)]
fn setup_content(app_config: &AppContext) -> crate::Result<Content<String>> {
  Ok(Content::Html(format!(
    "data:text/html,{}",
    urlencoding::encode(app_config.index)
  )))
}

// get the port for the embedded server
#[cfg(embedded_server)]
fn setup_port(app_config: &AppContext) -> crate::Result<(String, bool)> {
  let config = &app_config.config;
  match config.tauri.embedded_server.port {
    tauri_api::config::Port::Random => match get_available_port() {
      Some(available_port) => Ok((available_port.to_string(), true)),
      None => Ok(("0".to_string(), false)),
    },
    tauri_api::config::Port::Value(port) => {
      let port_valid = port_is_available(port);
      Ok((port.to_string(), port_valid))
    }
  }
}

// setup the server url for embedded server
#[cfg(embedded_server)]
fn setup_server_url(port: String, app_config: &AppContext) -> crate::Result<String> {
  let config = &app_config.config;
  let mut url = format!("{}:{}", config.tauri.embedded_server.host, port);
  if !url.starts_with("http") {
    url = format!("http://{}", url);
  }
  Ok(url)
}

// spawn the embedded server
#[cfg(embedded_server)]
fn spawn_server(server_url: String, app_config: &AppContext) -> crate::Result<()> {
  let assets = app_config.assets;
  spawn(move || {
    let server = tiny_http::Server::http(server_url.replace("http://", "").replace("https://", ""))
      .expect("Unable to spawn server");
    for request in server.incoming_requests() {
      let url = match request.url() {
        "/" => "/index.tauri.html",
        url => url,
      }
      .to_string();
      request
        .respond(crate::server::asset_response(&url, assets))
        .expect("unable to setup response");
    }
  });

  Ok(())
}

// spawn an updater process.
#[cfg(feature = "updater")]
fn spawn_updater() -> crate::Result<()> {
  spawn(|| {
    tauri_api::command::spawn_relative_command("updater".to_string(), Vec::new(), Stdio::inherit())
      .expect("Unable to spawn relative command");
  });
  Ok(())
}

pub fn init() -> String {
  #[cfg(not(event))]
  return String::from("");
  #[cfg(event)]
  return format!(
    "
      window['{queue}'] = [];
      window['{fn}'] = function (payload, salt, ignoreQueue) {{
      const listeners = (window['{listeners}'] && window['{listeners}'][payload.type]) || []
      if (!ignoreQueue && listeners.length === 0) {{
        window['{queue}'].push({{
          payload: payload,
          salt: salt
        }})
      }}

      if (listeners.length > 0) {{
        window.__TAURI__.promisified({{
          cmd: 'validateSalt',
          salt: salt
        }}).then(function () {{
          for (let i = listeners.length - 1; i >= 0; i--) {{
            const listener = listeners[i]
            if (listener.once)
              listeners.splice(i, 1)
            listener.handler(payload)
          }}
        }})
      }}
    }}
    ",
    fn = crate::event::emit_function_name(),
    queue = crate::event::event_queue_object_name(),
    listeners = crate::event::event_listeners_object_name()
  );
}

// build the webview struct
fn build_webview(
  application: &mut App,
  content: Content<String>,
  splashscreen_content: Option<Content<String>>,
) -> crate::Result<Webview<'_>> {
  let config = &application.ctx.config;
  let content_clone = match content {
    Content::Html(ref html) => Content::Html(html.clone()),
    Content::Url(ref url) => Content::Url(url.clone()),
  };
  let debug = cfg!(debug_assertions);
  // get properties from config struct
  let width = config.tauri.window.width;
  let height = config.tauri.window.height;
  let resizable = if config.tauri.window.resizable {
    SizeHint::NONE
  } else {
    SizeHint::FIXED
  };
  // let fullscreen = config.tauri.window.fullscreen;
  let title = config.tauri.window.title.clone().into_boxed_str();

  let has_splashscreen = splashscreen_content.is_some();
  let mut initialized_splashscreen = false;
  let url = match splashscreen_content {
    Some(Content::Html(s)) => s,
    _ => match content {
      Content::Html(s) => s,
      Content::Url(s) => s,
    },
  };

  let init = format!(
    r#"
      {event_init}
      if (window.__TAURI_INVOKE_HANDLER__) {{
        window.__TAURI_INVOKE_HANDLER__({{ cmd: "__initialized" }})
      }} else {{
        window.addEventListener('DOMContentLoaded', function () {{
          window.__TAURI_INVOKE_HANDLER__({{ cmd: "__initialized" }})
        }})
      }}
      {plugin_init}
    "#,
    event_init = init(),
    plugin_init = crate::plugin::init_script()
  );

  let mut webview = WebviewBuilder::new()
    .init(Box::leak(init.into_boxed_str()))
    .title(Box::leak(title))
    .width(width as usize)
    .height(height as usize)
    .resize(resizable)
    .debug(debug)
    .url(Box::leak(url.into_boxed_str()))
    .build();
  // TODO waiting for webview window API
  // webview.set_fullscreen(fullscreen);

  if has_splashscreen {
    let path = Path::new(&config.build.dist).join("tauri.js");
    let contents = fs::read_to_string(path)?;
    // inject the tauri.js entry point
    webview.dispatch(move |_webview| _webview.eval(&contents));
  }

  let mut w = webview.clone();
  webview.bind("__TAURI_INVOKE_HANDLER__", move |_, arg| {
    // transform `[payload]` to `payload`
    let arg = arg.chars().skip(1).take(arg.len() - 2).collect::<String>();
    if arg == r#"{"cmd":"__initialized"}"# {
      let source = if has_splashscreen && !initialized_splashscreen {
        initialized_splashscreen = true;
        "splashscreen"
      } else {
        "window-1"
      };
      application.run_setup(&mut w, source.to_string());
      if source == "window-1" {
        w.dispatch(|w| {
          crate::plugin::ready(w);
        });
      }
    } else if arg == r#"{"cmd":"closeSplashscreen"}"# {
      let content_href = match content_clone {
        Content::Html(ref html) => html,
        Content::Url(ref url) => url,
      };
      w.eval(&format!(r#"window.location.href = "{}""#, content_href));
    } else {
      let endpoint_handle = crate::endpoints::handle(&mut w, &arg, &application.ctx)
        .map_err(|tauri_handle_error| {
          let tauri_handle_error_str = tauri_handle_error.to_string();
          if tauri_handle_error_str.contains("unknown variant") {
            match application.run_invoke_handler(&mut w, &arg) {
              Ok(handled) => {
                if handled {
                  String::from("")
                } else {
                  tauri_handle_error_str
                }
              }
              Err(e) => e,
            }
          } else {
            tauri_handle_error_str
          }
        })
        .map_err(|app_handle_error| {
          if app_handle_error.contains("unknown variant") {
            match crate::plugin::extend_api(&mut w, &arg) {
              Ok(handled) => {
                if handled {
                  String::from("")
                } else {
                  app_handle_error
                }
              }
              Err(e) => e,
            }
          } else {
            app_handle_error
          }
        })
        .map_err(|e| e.replace("'", "\\'"));
      if let Err(handler_error_message) = endpoint_handle {
        if handler_error_message != "" {
          w.eval(&get_api_error_message(&arg, handler_error_message));
        }
      }
    }
  });

  Ok(webview)
}

// Formats an invoke handler error message to print to console.error
fn get_api_error_message(arg: &str, handler_error_message: String) -> String {
  format!(
    r#"console.error('failed to match a command for {}, {}')"#,
    arg.replace("'", "\\'"),
    handler_error_message
  )
}

#[cfg(test)]
mod test {
  use super::Content;
  use crate::AppContext;
  use crate::FromTauriConfig;
  use proptest::prelude::*;

  #[derive(FromTauriConfig)]
  #[tauri_config_path = "test/fixture/src-tauri/tauri.conf.json"]
  struct Config;

  #[test]
  fn check_setup_content() {
    let app_config = AppContext::new::<Config>().unwrap();
    let res = super::setup_content(&app_config);

    #[cfg(embedded_server)]
    match res {
      Ok(Content::Url(u)) => assert!(u.contains("http://")),
      _ => panic!("setup content failed"),
    }

    #[cfg(no_server)]
    match res {
      Ok(Content::Html(s)) => {
        assert_eq!(
          s,
          format!("data:text/html,{}", urlencoding::encode(app_config.index))
        );
      }
      _ => panic!("setup content failed"),
    }

    #[cfg(dev)]
    {
      let config = &app_config.config;
      match res {
        Ok(Content::Url(dp)) => assert_eq!(dp, config.build.dev_path),
        Ok(Content::Html(s)) => {
          assert_eq!(
            s,
            format!("data:text/html,{}", urlencoding::encode(app_config.index))
          );
        }
        _ => panic!("setup content failed"),
      }
    }
  }

  #[cfg(embedded_server)]
  #[test]
  fn check_setup_port() {
    let app_config = AppContext::new::<Config>().unwrap();
    let res = super::setup_port(&app_config);
    match res {
      Ok((_s, _b)) => {}
      _ => panic!("setup port failed"),
    }
  }

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]
    #[cfg(embedded_server)]
    #[test]
    fn check_server_url(port in (any::<u32>().prop_map(|v| v.to_string()))) {
      let p = port.clone();
      let app_config = AppContext::new::<Config>().unwrap();

      let res = super::setup_server_url(port, &app_config);

      match res {
        Ok(url) => assert!(url.contains(&p)),
        Err(e) => panic!("setup_server_url Err {:?}", e.to_string())
      }
    }
  }
}
