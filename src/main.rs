mod cli;
mod client;
mod output;

use anyhow::Result;
use clap::Parser;
use serde_json::json;

use cli::{Cli, Commands};
use client::ErebyxClient;
use output::{print_error, print_response};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        print_error(&format!("{:#}", e));
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let json_mode = cli.json;

    match cli.command {
        Commands::Health => {
            let client = ErebyxClient::new()?;
            let result = client.health().await?;
            print_response(&result, false, json_mode);
        }

        Commands::RestoreIdentity {
            limit,
            include_guide,
            detail,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({});

            if let Some(limit) = limit {
                args["limit"] = json!(limit);
            }
            if include_guide {
                args["include_guide"] = json!(true);
            }
            if let Some(detail) = detail {
                args["detail"] = json!(detail.to_string());
            }

            let resp = client.call_tool("restore_identity", args).await?;
            print_response(&resp.content, resp.is_error, json_mode);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::LoadContext { anchors, mode } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({});

            if let Some(anchors) = anchors {
                args["anchors"] = json!(anchors);
            }
            if let Some(mode) = mode {
                args["mode"] = json!(mode.to_string());
            }

            let resp = client.call_tool("load_context", args).await?;
            print_response(&resp.content, resp.is_error, json_mode);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::Save {
            content,
            category,
            title,
            anchors,
            importance,
            memory_type,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({
                "content": content,
                "category": category,
            });

            if let Some(title) = title {
                args["title"] = json!(title);
            }
            if let Some(anchors) = anchors {
                args["anchors"] = json!(anchors);
            }
            if let Some(importance) = importance {
                args["importance"] = json!(importance);
            }
            if let Some(memory_type) = memory_type {
                args["type"] = json!(memory_type.to_string());
            }

            let resp = client.call_tool("save", args).await?;
            print_response(&resp.content, resp.is_error, json_mode);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::Remember {
            query,
            anchors,
            limit,
            time_range,
            ids,
            generative,
            types,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({
                "query": query,
                "limit": limit,
            });

            if let Some(anchors) = anchors {
                args["hint_anchors"] = json!(anchors);
            }
            if let Some(time_range) = time_range {
                args["time_range"] = json!(time_range.to_string());
            }
            if let Some(ids) = ids {
                args["ids"] = json!(ids);
            }
            if let Some(generative) = generative {
                args["generative"] = json!(generative.to_string());
            }
            if let Some(types) = types {
                args["types"] = json!(types);
            }

            let resp = client.call_tool("remember", args).await?;
            print_response(&resp.content, resp.is_error, json_mode);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::Evolve {
            target_id,
            target_type,
            intent,
            trigger,
            new_insight,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({
                "target_id": target_id,
                "target_type": target_type,
                "intent": intent,
                "trigger": trigger,
            });

            if let Some(insight) = new_insight {
                args["new_insight"] = json!(insight);
            }

            let resp = client.call_tool("evolve", args).await?;
            print_response(&resp.content, resp.is_error, json_mode);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::Learn {
            experience,
            outcome,
            insight,
            domain,
            skill,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({});

            if let Some(experience) = experience {
                args["experience"] = json!(experience);
            }
            if let Some(outcome) = outcome {
                args["outcome"] = json!(outcome);
            }
            if let Some(insight) = insight {
                args["insight"] = json!(insight);
            }
            if let Some(domain) = domain {
                args["domain"] = json!(domain);
            }
            if let Some(skill) = skill {
                args["skill"] = json!(skill);
            }

            let resp = client.call_tool("learn", args).await?;
            print_response(&resp.content, resp.is_error, json_mode);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::WrapUp {
            what_we_built,
            whats_next,
            anchors,
            energy,
            diary,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({
                "what_we_built": what_we_built,
                "whats_next": whats_next,
            });

            if let Some(anchors) = anchors {
                args["anchors"] = json!(anchors);
            }
            if let Some(energy) = energy {
                args["energy"] = json!(energy);
            }
            if let Some(diary) = diary {
                args["diary"] = json!(diary);
            }

            let resp = client.call_tool("wrap_up", args).await?;
            print_response(&resp.content, resp.is_error, json_mode);
            if resp.is_error {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
