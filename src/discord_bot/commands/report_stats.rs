use serenity::framework::standard::{macros::command, Args, CommandError, CommandResult};
use serenity::model::prelude::*;
use serenity::prelude::*;

use super::super::start_bot::LogAnalysisClientContainer;
use crate::fight_analysis::analyse_fight::{
    analyse_fights_by_name, convert_report_code, get_report_stats, summarise_report, AnalysisError,
};

use futures::select;
use std::time;
use tokio::time::delay_for;

use log::{debug, info, trace};

#[command]
#[description = "Gets statistics for progression on a specified fight in the provided FFLogs report"]
#[bucket = "fflogs_api"]
#[aliases("proggies")]
pub async fn fight_stats(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let tgt_report = args.single::<String>()?;
    let fight_name = args.single_quoted::<String>()?;
    debug!(
        "Got request to fetch {} fights from report {}",
        fight_name, tgt_report
    );
    let _opts = args.remains();
    let data = ctx.data.read().await;
    let analysis_client = data
        .get::<LogAnalysisClientContainer>()
        .ok_or(CommandError(
            "Failed to fetch fflogs api client".to_string(),
        ))?;

    let report_code: String = handle_errors(ctx, msg, convert_report_code(tgt_report)).await?;
    let analysis_fut = select! {
        analysis = analyse_fights_by_name(report_code.to_string(), fight_name, analysis_client).fuse() => Some(analysis),
        _ = show_typing(ctx, msg).fuse() => None,
    }
    .ok_or(CommandError(
        "Something went horribly wrong. Discord API calls failed.".to_string(),
    ))?;

    let analysis = handle_errors(ctx, msg, analysis_fut).await;
    debug!(
        "Finished report analysis for report {}, got result '{:?}'",
        report_code, analysis
    );
    let report_stats = get_report_stats(analysis?);
    trace!(
        "Calculated report stats for report {}, got result '{:?}'",
        report_code,
        report_stats
    );
    let report_summary = summarise_report(report_stats);
    trace!(
        "Calculated report summary for report {}, got result '{:?}'",
        report_code,
        report_summary
    );

    msg.reply(ctx, format!("\n{}", report_summary)).await?;

    Ok(())
}

async fn show_typing(ctx: &Context, msg: &Message) -> CommandResult {
    loop {
        msg.channel_id.broadcast_typing(ctx).await?;
        info!("Sent typing notification to discord.");
        delay_for(time::Duration::from_secs(4)).await;
    }
}

async fn handle_errors<T>(
    ctx: &Context,
    msg: &Message,
    res: Result<T, AnalysisError>,
) -> Result<T, CommandError> {
    match res {
        Ok(result) => Ok(result),
        Err(e) => {
            let com_err = format!(
                "Encountered error processing message '{:?}': {:?}",
                msg.content, e
            );
            let user_err_msg = format!("Uh-oh, something went wrong:\n{}", e);
            msg.reply(ctx, user_err_msg).await?;
            Err(CommandError(com_err))
        }
    }
}
