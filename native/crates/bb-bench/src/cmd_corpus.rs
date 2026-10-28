//! Builds or inspects the cached synthetic corpus without running anything. Spec: Cli/CorpusCommand.cs.
//!
//! Only ever the folder `corpus::default_root` names (or one a caller names whose own name carries
//! `bertbrowser-bench`): a rebuild deletes the tree it is about to rewrite, so it is refused anywhere
//! that is not recognisably a bench corpus.

use std::path::PathBuf;

use crate::args::{ArgList, Fail, unknown};
use crate::corpus;

pub fn execute(mut args: ArgList) -> Result<i32, Fail> {
    let mut scale = 1;
    let mut rebuild = false;
    let mut stats = false;
    let mut root: Option<PathBuf> = None;

    while let Some(arg) = args.next_arg() {
        match arg.as_str() {
            "--scale" => scale = args.int(&arg)?.max(1),
            "--rebuild" => rebuild = true,
            "--stats" => stats = true,
            "--root" => root = Some(PathBuf::from(args.value(&arg)?)),
            "--repo" => {
                args.value(&arg)?;
            }
            _ => return Err(unknown("corpus", &arg)),
        }
    }

    let root = root.unwrap_or_else(corpus::default_root);
    let say = |line: &str| println!("{line}");
    corpus::ensure_all(&root, scale, &say, rebuild)?;

    println!("database: {}", corpus::db_path(&root, scale).display());
    println!("tree:     {}", corpus::tree_root(&root, scale).display());
    if stats {
        println!("contents: {}", corpus::describe(&root, scale));
    }
    Ok(0)
}
