//! Projects and the project screen (`Projects.ps1`, `Invoke-ProjectScreen`, `Get-ProjectFrame`):
//! the registry of directories Claude Code has sessions in, the start selection, and the screen
//! that picks where a session runs and what it does there.

mod paths;
mod registry;
mod screen;
mod select;
mod view;
mod win;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::frame::Style;
use crate::launch::LaunchState;
use crate::ui::{Exhausted, Term};

pub use paths::project_key;

/// One project row (`Get-ProjectRegistry`): one real directory, every slug folder naming it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The directory's leaf name (`C:` for a drive root).
    pub name: String,
    /// The cwd the newest transcript recorded.
    pub path: String,
    /// The slug of the newest activity.
    pub slug: String,
    /// Every slug of this directory: what the session picker scopes on.
    pub slugs: Vec<String>,
    /// The worktree name a `--claude-worktrees-` slug carries.
    pub worktree: Option<String>,
    /// Unix milliseconds of the newest interactive transcript (else the newest).
    pub last_activity_ms: Option<i64>,
    /// A scratch directory, or a folder only headless runs used that is no repository root: known,
    /// listed only by a filter or as the cwd.
    pub hidden: bool,
}

/// What the project screen returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectChoice {
    pub path: String,
    /// A launch-mode name (`launch::LAUNCH_MODES`).
    pub action: String,
    pub slug: String,
    pub slugs: Vec<String>,
    /// enter | hotkey | mouse.
    pub how: String,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// `Get-ProjectRegistry`. The projects root is `CLAUDE_AUTO_PROJECTS_ROOT`, else
/// `~\.claude\projects`; the cache is `cache_path` (preview: its own per-PID file), else
/// `claude-auto-projects.json` beside the root. `profile_roots`: every account root a job tmp
/// directory can sit under. Never fails: an unreadable root is an empty list.
pub fn project_registry(cache_path: Option<&Path>, profile_roots: &[String]) -> Vec<Project> {
    let home = home();
    let root = std::env::var_os("CLAUDE_AUTO_PROJECTS_ROOT")
        .filter(|v| !v.is_empty())
        .map_or_else(|| home.join(".claude").join("projects"), PathBuf::from);
    let cache = cache_path.map_or_else(
        || root.parent().unwrap_or(&root).join("claude-auto-projects.json"),
        Path::to_path_buf,
    );
    let temp = paths::temp_roots(&paths::default_temp_candidates(), &home.to_string_lossy());
    registry::build_registry(&registry::RegistryInput {
        projects_root: &root,
        cache_path: &cache,
        temp_roots: &temp,
        profile_roots,
    })
}

/// The launch context the start rule is re-applied with on a tab switch.
#[derive(Debug, Clone)]
struct StartContext {
    cwd: String,
    projects: Vec<Project>,
}

thread_local! {
    /// `$script:LaunchStartContext`: `update_start_selection` is called from `prefs::switch_account`
    /// with the state alone, so the context recorded at start lives here, one per UI thread.
    static START: RefCell<Option<StartContext>> = const { RefCell::new(None) };
}

fn apply_start(state: &mut LaunchState, cwd: &str, projects: &[Project]) -> &'static str {
    let (path, source) = select::resolve_start(cwd, &state.project, projects);
    state.project = path.unwrap_or_default();
    state.project_slug.clear();
    state.project_slugs.clear();
    if !state.project.is_empty() {
        let key = project_key(&state.project);
        let hits: Vec<&Project> = projects.iter().filter(|p| project_key(&p.path) == key).collect();
        if let Some(first) = hits.first() {
            state.project_slug.clone_from(&first.slug);
            state.project_slugs = hits
                .iter()
                .flat_map(|p| {
                    if p.slugs.is_empty() {
                        vec![p.slug.clone()]
                    } else {
                        p.slugs.clone()
                    }
                })
                .collect();
        }
    }
    source.as_str()
}

/// `Set-LaunchStartProject`: the cwd when it is a known project or holds a `.git`, else the
/// remembered project (`state.project`) when it is still there, else none. Sets the project, its
/// slug and every slug of its directory; records the context for `update_start_selection`.
/// Returns the source: cwd | remembered | none.
pub fn set_launch_start_project(state: &mut LaunchState, cwd: &Path, projects: &[Project]) -> String {
    let cwd = cwd.to_string_lossy().into_owned();
    let source = apply_start(state, &cwd, projects);
    START.with(|s| {
        *s.borrow_mut() = Some(StartContext {
            cwd,
            projects: projects.to_vec(),
        });
    });
    source.into()
}

/// `Update-LaunchStartSelection`: re-decides the start selection after a tab switch replaced the
/// remembered project. A no-op when no start selection ran.
pub fn update_start_selection(state: &mut LaunchState) {
    START.with(|s| {
        if let Some(ctx) = s.borrow().as_ref() {
            apply_start(state, &ctx.cwd, &ctx.projects);
        }
    });
}

/// `Remove-StalePreviewProjectsCache`: the per-PID preview caches a killed run left, older than a
/// day, by their exact name only.
pub fn remove_stale_preview_cache(dir: &Path) {
    registry::remove_stale_preview_cache_at(dir, SystemTime::now());
}

/// `Invoke-ProjectScreen`: where the session runs and what it does there. `None` = Escape, back to
/// the launch screen. The cursor opens on the current directory and the action field on `new`.
pub fn run_project_screen(term: &mut dyn Term, projects: &[Project], cwd: &Path, style: Style) -> Result<Option<ProjectChoice>, Exhausted> {
    let clock = || registry::unix_ms(SystemTime::now());
    screen::run_screen(term, projects, cwd, "new", style, &clock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::registry::tests::Tree;
    use crate::projects::select::tests::proj;

    fn state() -> LaunchState {
        LaunchState::fresh("work")
    }

    #[test]
    fn set_launch_start_project_applies_path_and_slugs() {
        let ps = vec![proj("SLS-A", r"C:\w\alpha", "alpha")];
        let mut s = state();
        s.project_slug = "stale".into();
        assert_eq!(set_launch_start_project(&mut s, Path::new(r"C:\w\alpha"), &ps), "cwd");
        assert_eq!((s.project.as_str(), s.project_slug.as_str()), (r"C:\w\alpha", "SLS-A"));
        let mut n = state();
        assert_eq!(
            set_launch_start_project(&mut n, Path::new(r"C:\Users\someone\Desktop"), &ps),
            "none"
        );
        assert_eq!((n.project.as_str(), n.project_slug.as_str()), ("", ""));
        let t = Tree::new("startgit");
        let git = t.dir(r"repo\.git");
        let repo = Path::new(&git).parent().unwrap().to_path_buf();
        let mut g = state();
        assert_eq!(set_launch_start_project(&mut g, &repo, &ps), "cwd");
        assert_eq!((g.project.as_str(), g.project_slug.as_str()), (repo.to_str().unwrap(), ""));
    }

    #[test]
    fn the_start_state_carries_every_slug_of_the_directory() {
        let mut p = proj("C--tmp-Shared", r"C:\tmp\Shared", "Shared");
        p.slugs = vec!["C--tmp-Shared".into(), "C--tmp-Shared-alt".into()];
        let mut s = state();
        set_launch_start_project(&mut s, Path::new(r"C:\tmp\Shared"), &[p]);
        assert_eq!(s.project_slugs, ["C--tmp-Shared", "C--tmp-Shared-alt"]);
    }

    #[test]
    fn a_tab_switch_reapplies_the_cwd_rule() {
        let ps = vec![proj("A", r"C:\w\alpha", "alpha")];
        let t = Tree::new("tabswitch");
        let remembered = t.dir("remembered");
        let mut s = state();
        update_start_selection(&mut s);
        assert_eq!(s.project, "", "no recorded context: a no-op");
        set_launch_start_project(&mut s, Path::new(r"C:\w\alpha"), &ps);
        s.project.clone_from(&remembered);
        update_start_selection(&mut s);
        assert_eq!(
            s.project, r"C:\w\alpha",
            "the cwd outranks the arriving account's remembered project"
        );
        let mut u = state();
        set_launch_start_project(&mut u, Path::new(r"C:\Users\someone\Desktop"), &ps);
        u.project.clone_from(&remembered);
        update_start_selection(&mut u);
        assert_eq!(u.project, remembered, "a neutral cwd takes the arriving account's project");
    }
}
