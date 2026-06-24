use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored_json::to_colored_json_auto;
use reqwest::Method;
use serde_json::Value;
use std::io::{self, Read};

mod client;
mod discover;
mod tui;

use client::LocalAIClient;

#[derive(Parser)]
#[command(name = "localai")]
#[command(about = "CLI for LocalAI APIs")]
#[command(version)]
struct Cli {
    #[arg(long, env = "LOCALAI_URL", default_value = "http://localhost:8080")]
    base_url: String,

    #[arg(long, env = "LOCALAI_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Model management
    Models {
        #[command(subcommand)]
        command: ModelCommands,
    },
    /// Chat completions
    Chat {
        /// JSON request body or @file. If omitted, reads from stdin.
        #[arg(short, long)]
        body: Option<String>,
        /// Model name (convenience shorthand)
        #[arg(short, long)]
        model: Option<String>,
        /// User message (convenience shorthand, repeatable)
        #[arg(long)]
        message: Vec<String>,
        /// Stream response
        #[arg(long)]
        stream: bool,
    },
    /// Text completions
    Completions {
        /// JSON request body or @file. If omitted, reads from stdin.
        #[arg(short, long)]
        body: Option<String>,
        /// Model name
        #[arg(short, long)]
        model: Option<String>,
        /// Prompt text
        #[arg(short, long)]
        prompt: Option<String>,
    },
    /// Embeddings
    Embeddings {
        /// JSON request body or @file. If omitted, reads from stdin.
        #[arg(short, long)]
        body: Option<String>,
        /// Model name
        #[arg(short, long)]
        model: Option<String>,
        /// Input text
        #[arg(short, long)]
        input: Option<String>,
    },
    /// Image generation
    Images {
        #[command(subcommand)]
        command: ImageCommands,
    },
    /// Audio operations
    Audio {
        #[command(subcommand)]
        command: AudioCommands,
    },
    /// Backend management
    Backends {
        #[command(subcommand)]
        command: BackendCommands,
    },
    /// Raw API request
    Request {
        /// HTTP method
        method: String,
        /// API path (e.g. /v1/models)
        path: String,
        /// JSON body or @file
        #[arg(short, long)]
        body: Option<String>,
        /// Query parameters as key=value (repeatable)
        #[arg(short, long)]
        query: Vec<String>,
    },
    /// List API endpoints from doc.json
    Endpoints {
        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,
    },
    /// List API tags
    Tags,
    /// Interactive terminal UI for browsing and calling the APIs
    Tui,
}

#[derive(Subcommand)]
enum ModelCommands {
    /// List available models
    List,
    /// List available (installable) models
    Available,
    /// Apply a model configuration
    Apply {
        /// JSON body or @file
        #[arg(short, long)]
        body: String,
    },
    /// Delete a model
    Delete { name: String },
    /// List model galleries
    Galleries,
    /// List model jobs
    Jobs,
    /// Get job status
    Job { uuid: String },
}

#[derive(Subcommand)]
enum ImageCommands {
    /// Generate an image
    Generate {
        /// JSON body or @file. If omitted, reads from stdin.
        #[arg(short, long)]
        body: Option<String>,
        /// Model name
        #[arg(short, long)]
        model: Option<String>,
        /// Prompt
        #[arg(short, long)]
        prompt: Option<String>,
    },
    /// Inpaint an image
    Inpaint {
        /// JSON body or @file. If omitted, reads from stdin.
        #[arg(short, long)]
        body: Option<String>,
    },
}

#[derive(Subcommand)]
enum AudioCommands {
    /// Transcribe audio
    Transcribe {
        /// Audio file path
        #[arg(short, long)]
        file: String,
        /// Model name
        #[arg(short, long)]
        model: String,
        /// Language hint
        #[arg(short, long)]
        language: Option<String>,
        /// Response format: json, text, srt, vtt, verbose_json
        #[arg(short, long)]
        response_format: Option<String>,
    },
    /// Generate speech
    Speech {
        /// JSON body or @file. If omitted, reads from stdin.
        #[arg(short, long)]
        body: Option<String>,
        /// Model name
        #[arg(short, long)]
        model: Option<String>,
        /// Voice ID
        #[arg(short, long)]
        voice: Option<String>,
        /// Input text
        #[arg(short, long)]
        input: Option<String>,
    },
    /// Transform audio
    Transform {
        /// Audio file path
        #[arg(short, long)]
        file: String,
        /// Model name
        #[arg(short, long)]
        model: String,
        /// Response format: wav, mp3, ogg, flac
        #[arg(short, long)]
        response_format: Option<String>,
    },
}

#[derive(Subcommand)]
enum BackendCommands {
    /// List installed backends
    List,
    /// List available backends
    Available,
    /// List backend jobs
    Jobs,
    /// Get backend job status
    Job { uuid: String },
    /// List backend galleries
    Galleries,
    /// List known backends
    Known,
}

fn parse_body(input: Option<String>) -> Result<Option<Value>> {
    let text = match input {
        Some(s) if s.starts_with('@') => std::fs::read_to_string(&s[1..])
            .with_context(|| format!("reading body file: {}", &s[1..]))?,
        Some(s) => s,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            if buf.trim().is_empty() {
                return Ok(None);
            }
            buf
        }
    };
    if text.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            serde_json::from_str(&text).with_context(|| "parsing JSON body")?,
        ))
    }
}

fn build_body(
    body: Option<String>,
    overrides: Vec<(&str, Option<String>)>,
) -> Result<Option<Value>> {
    let mut value = parse_body(body)?.unwrap_or_else(|| Value::Object(serde_json::Map::new()));

    if let Value::Object(ref mut map) = value {
        for (key, opt_val) in overrides {
            if let Some(val) = opt_val {
                map.insert(key.to_string(), Value::String(val));
            }
        }
    }

    if value.as_object().map(|m| m.is_empty()).unwrap_or(false) {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", to_colored_json_auto(value)?);
    Ok(())
}

fn parse_queries(queries: &[String]) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    for q in queries {
        let Some((k, v)) = q.split_once('=') else {
            return Err(anyhow::anyhow!("Query parameter must be key=value: {q}"));
        };
        result.push((k.to_string(), v.to_string()));
    }
    Ok(result)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = LocalAIClient::new(cli.base_url, cli.api_key);

    match cli.command {
        Commands::Models { command } => match command {
            ModelCommands::List => {
                let resp = client
                    .request_json(Method::GET, "/v1/models", None, None)
                    .await?;
                print_json(&resp)?;
            }
            ModelCommands::Available => {
                let resp = client
                    .request_json(Method::GET, "/models/available", None, None)
                    .await?;
                print_json(&resp)?;
            }
            ModelCommands::Apply { body } => {
                let body = parse_body(Some(body))?;
                let resp = client
                    .request_json(Method::POST, "/models/apply", None, body)
                    .await?;
                print_json(&resp)?;
            }
            ModelCommands::Delete { name } => {
                let resp = client
                    .request_json(Method::POST, &format!("/models/delete/{name}"), None, None)
                    .await?;
                print_json(&resp)?;
            }
            ModelCommands::Galleries => {
                let resp = client
                    .request_json(Method::GET, "/models/galleries", None, None)
                    .await?;
                print_json(&resp)?;
            }
            ModelCommands::Jobs => {
                let resp = client
                    .request_json(Method::GET, "/models/jobs", None, None)
                    .await?;
                print_json(&resp)?;
            }
            ModelCommands::Job { uuid } => {
                let resp = client
                    .request_json(Method::GET, &format!("/models/jobs/{uuid}"), None, None)
                    .await?;
                print_json(&resp)?;
            }
        },

        Commands::Chat {
            body,
            model,
            message,
            stream,
        } => {
            let mut body = build_body(body, vec![("model", model)])?
                .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

            if let Value::Object(ref mut map) = body {
                if !message.is_empty() && !map.contains_key("messages") {
                    let messages: Vec<Value> = message
                        .into_iter()
                        .map(|m| {
                            serde_json::json!({
                                "role": "user",
                                "content": m
                            })
                        })
                        .collect();
                    map.insert("messages".to_string(), Value::Array(messages));
                }
                if stream {
                    map.insert("stream".to_string(), Value::Bool(true));
                }
            }

            let resp = client
                .request_json(Method::POST, "/v1/chat/completions", None, Some(body))
                .await?;
            print_json(&resp)?;
        }

        Commands::Completions {
            body,
            model,
            prompt,
        } => {
            let body = build_body(body, vec![("model", model), ("prompt", prompt)])?;
            let resp = client
                .request_json(Method::POST, "/v1/completions", None, body)
                .await?;
            print_json(&resp)?;
        }

        Commands::Embeddings { body, model, input } => {
            let body = build_body(body, vec![("model", model), ("input", input)])?;
            let resp = client
                .request_json(Method::POST, "/v1/embeddings", None, body)
                .await?;
            print_json(&resp)?;
        }

        Commands::Images { command } => match command {
            ImageCommands::Generate {
                body,
                model,
                prompt,
            } => {
                let body = build_body(body, vec![("model", model), ("prompt", prompt)])?;
                let resp = client
                    .request_json(Method::POST, "/v1/images/generations", None, body)
                    .await?;
                print_json(&resp)?;
            }
            ImageCommands::Inpaint { body } => {
                let body = parse_body(body)?;
                let resp = client
                    .request_json(Method::POST, "/v1/images/inpainting", None, body)
                    .await?;
                print_json(&resp)?;
            }
        },

        Commands::Audio { command } => match command {
            AudioCommands::Transcribe {
                file,
                model,
                language,
                response_format,
            } => {
                let file_bytes = tokio::fs::read(&file)
                    .await
                    .with_context(|| format!("reading audio file: {file}"))?;
                let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file.clone());
                let mut form = reqwest::multipart::Form::new()
                    .part("file", part)
                    .text("model", model);
                if let Some(lang) = language {
                    form = form.text("language", lang);
                }
                if let Some(fmt) = response_format {
                    form = form.text("response_format", fmt);
                }
                let resp = client
                    .request_multipart("/v1/audio/transcriptions", form)
                    .await?;
                print_json(&resp)?;
            }
            AudioCommands::Speech {
                body,
                model,
                voice,
                input,
            } => {
                let body = build_body(
                    body,
                    vec![("model", model), ("voice", voice), ("input", input)],
                )?;
                let resp = client
                    .request_json(Method::POST, "/v1/audio/speech", None, body)
                    .await?;
                print_json(&resp)?;
            }
            AudioCommands::Transform {
                file,
                model,
                response_format,
            } => {
                let file_bytes = tokio::fs::read(&file)
                    .await
                    .with_context(|| format!("reading audio file: {file}"))?;
                let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file.clone());
                let mut form = reqwest::multipart::Form::new()
                    .part("audio", part)
                    .text("model", model);
                if let Some(fmt) = response_format {
                    form = form.text("response_format", fmt);
                }
                let resp = client.request_multipart("/audio/transform", form).await?;
                print_json(&resp)?;
            }
        },

        Commands::Backends { command } => match command {
            BackendCommands::List => {
                let resp = client
                    .request_json(Method::GET, "/backends", None, None)
                    .await?;
                print_json(&resp)?;
            }
            BackendCommands::Available => {
                let resp = client
                    .request_json(Method::GET, "/backends/available", None, None)
                    .await?;
                print_json(&resp)?;
            }
            BackendCommands::Jobs => {
                let resp = client
                    .request_json(Method::GET, "/backends/jobs", None, None)
                    .await?;
                print_json(&resp)?;
            }
            BackendCommands::Job { uuid } => {
                let resp = client
                    .request_json(Method::GET, &format!("/backends/jobs/{uuid}"), None, None)
                    .await?;
                print_json(&resp)?;
            }
            BackendCommands::Galleries => {
                let resp = client
                    .request_json(Method::GET, "/backends/galleries", None, None)
                    .await?;
                print_json(&resp)?;
            }
            BackendCommands::Known => {
                let resp = client
                    .request_json(Method::GET, "/backends/known", None, None)
                    .await?;
                print_json(&resp)?;
            }
        },

        Commands::Request {
            method,
            path,
            body,
            query,
        } => {
            let method = Method::from_bytes(method.as_bytes())
                .with_context(|| format!("invalid HTTP method: {method}"))?;
            let body = parse_body(body)?;
            let query = if query.is_empty() {
                None
            } else {
                Some(parse_queries(&query)?)
            };
            let resp = client.request_json(method, &path, query, body).await?;
            print_json(&resp)?;
        }

        Commands::Endpoints { tag } => {
            discover::list_endpoints(tag.as_deref())?;
        }

        Commands::Tags => {
            discover::list_tags()?;
        }

        Commands::Tui => {
            tui::run(client).await?;
        }
    }

    Ok(())
}
