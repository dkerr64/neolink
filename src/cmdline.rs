use clap::Parser;
use std::path::PathBuf;
use std::str::FromStr;

/// A standards-compliant bridge to Reolink IP cameras
///
/// Neolink is free software released under the GNU AGPL v3.
/// You can find its source code at https://github.com/thirtythreeforty/neolink
#[derive(Parser, Debug)]
#[command(name = "neolink", arg_required_else_help = true)]
pub struct Opt {
    #[arg(short, long, global = true, value_parser = PathBuf::from_str)]
    pub config: Option<PathBuf>,
    #[structopt(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(Parser, Debug)]
pub enum Command {
    Rtsp(super::rtsp::Opt),
    StatusLight(super::statusled::Opt),
    Reboot(super::reboot::Opt),
    Pir(super::pir::Opt),
    Talk(super::talk::Opt),
    Mqtt(super::mqtt::Opt),
    Image(super::image::Opt),
}
