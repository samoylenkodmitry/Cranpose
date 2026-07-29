#[path = "../robot-runners/liquid_cheatsheets/mod.rs"]
mod liquid_cheatsheets;

fn main() -> anyhow::Result<()> {
    liquid_cheatsheets::run(liquid_cheatsheets::Case::OnWhiteClick)
}
