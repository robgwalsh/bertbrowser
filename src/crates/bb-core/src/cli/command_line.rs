//! Parses BertBrowser's command line. Pure: it never touches the filesystem. Shared by the process's
//! own startup arguments and by the single-instance pipe, so there is one grammar. Understands
//! Explorer's `/select,<path>` convention, which arrives as one argv token or two. Spec: Core/Cli/CommandLine.cs.

use super::{CommandLineRequest, OpenIn, OpenTarget};

pub struct CommandLine;

impl CommandLine {
    const SELECT_PREFIX: &'static str = "/select,";

    pub fn parse<S: AsRef<str>>(args: &[S]) -> CommandLineRequest {
        let mut request = CommandLineRequest::default();
        if args.is_empty() {
            return request;
        }
        let mut mode_chosen = false;
        let mut select_next = false;

        for raw in args {
            let arg = raw.as_ref().trim();
            if arg.is_empty() {
                continue;
            }

            // Explorer's convention. "/select,C:\dir\file.txt" is one token; "/select," followed by
            // the path is what it emits when a space follows the comma.
            if arg.len() >= Self::SELECT_PREFIX.len()
                && arg[..Self::SELECT_PREFIX.len()].eq_ignore_ascii_case(Self::SELECT_PREFIX)
            {
                let rest = &arg[Self::SELECT_PREFIX.len()..];
                if rest.is_empty() {
                    select_next = true;
                } else {
                    request.targets.push(OpenTarget::new(repair(rest), true));
                }
                continue;
            }

            if matches(arg, "--select", "-s") {
                select_next = true;
                continue;
            }
            if matches(arg, "--new-tab", "-t") {
                choose(&mut request, &mut mode_chosen, OpenIn::NewTab);
                continue;
            }
            if matches(arg, "--new-pane", "-p") {
                choose(&mut request, &mut mode_chosen, OpenIn::NewPane);
                continue;
            }

            if is_option(arg) {
                // Never fall through to "treat it as a path": a mistyped flag opening the user's
                // profile folder is worse than an error.
                request.errors.push(format!("Unrecognized option '{arg}'."));
                continue;
            }

            request
                .targets
                .push(OpenTarget::new(repair(arg), select_next));
            select_next = false;
        }

        if select_next {
            request
                .errors
                .push("'/select' expects a path after it.".to_string());
        }
        request
    }
}

fn choose(request: &mut CommandLineRequest, mode_chosen: &mut bool, chosen: OpenIn) {
    // Two contradictory requests is a mistake, and picking one silently would make it look like
    // the flag was ignored.
    if *mode_chosen && request.mode != chosen {
        request
            .errors
            .push("Choose either --new-tab or --new-pane, not both.".to_string());
        return;
    }
    request.mode = chosen;
    *mode_chosen = true;
}

fn matches(arg: &str, long: &str, short: &str) -> bool {
    arg.eq_ignore_ascii_case(long) || arg.eq_ignore_ascii_case(short)
}

/// A leading `-` or `/` means a switch. A path never starts with either on Windows.
fn is_option(arg: &str) -> bool {
    arg.starts_with('-') || arg.starts_with('/')
}

/// Undoes Windows' backslash-before-quote mangling: `"C:\Dir\"` reaches argv as `C:\Dir"`.
fn repair(path: &str) -> &str {
    path.trim_end_matches('"')
}
