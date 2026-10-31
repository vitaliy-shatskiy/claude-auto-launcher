//! What the project screen lists and where the launch starts (`Select-ProjectMatch`,
//! `Get-ProjectRows`, `Get-ProjectCount`, `Resolve-StartProject`). Pure but for the existence checks
//! of the start selection.

use std::path::Path;

use super::Project;
use super::paths::{DriveCache, path_present, project_key};

/// `Select-ProjectMatch`: a literal, case-insensitive substring of "name path worktree" (a pasted
/// path must match as typed). No filter: the hidden rows are left out, except the cwd's own. A
/// filter: the visible matches first, the hidden ones after, each in registry order.
pub(crate) fn select_match<'a>(projects: &'a [Project], filter: &str, cwd: &str) -> Vec<&'a Project> {
    let f = filter.trim();
    if f.is_empty() {
        let cwd_key = project_key(cwd);
        return projects
            .iter()
            .filter(|p| !p.hidden || (!cwd_key.is_empty() && project_key(&p.path) == cwd_key))
            .collect();
    }
    let needle = f.to_lowercase();
    let hits = projects.iter().filter(|p| {
        format!("{} {} {}", p.name, p.path, p.worktree.as_deref().unwrap_or_default())
            .to_lowercase()
            .contains(&needle)
    });
    let (hidden, shown): (Vec<&Project>, Vec<&Project>) = hits.partition(|p| p.hidden);
    shown.into_iter().chain(hidden).collect()
}

/// The kind of a project-screen row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowKind {
    Cwd,
    Project,
    Path,
}

/// One project-screen row in cursor order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub kind: RowKind,
    pub name: String,
    pub path: String,
    pub last_activity_ms: Option<i64>,
    pub worktree: bool,
    pub slug: String,
    pub slugs: Vec<String>,
    /// The cwd row: its project is among the filter's matches.
    pub matched: bool,
}

fn all_slugs(p: &Project) -> Vec<String> {
    if p.slugs.is_empty() {
        vec![p.slug.clone()]
    } else {
        p.slugs.clone()
    }
}

/// `Get-ProjectRows`: the cwd row, the matches, the free-path row - the ONE builder the frame draws
/// and the screen picks from. A cwd that is a known project carries it and leaves the list.
pub(crate) fn project_rows(projects: &[Project], filter: &str, cwd: &str) -> Vec<Row> {
    let cwd_key = project_key(cwd);
    let here = (!cwd_key.is_empty())
        .then(|| projects.iter().find(|p| project_key(&p.path) == cwd_key))
        .flatten();
    let matched = select_match(projects, filter, cwd);
    let cwd_row = match here {
        Some(h) => Row {
            kind: RowKind::Cwd,
            name: format!("{} (current directory)", h.name),
            path: cwd.to_owned(),
            last_activity_ms: h.last_activity_ms,
            worktree: h.worktree.is_some(),
            slug: h.slug.clone(),
            slugs: all_slugs(h),
            matched: !filter.is_empty() && matched.iter().any(|p| project_key(&p.path) == cwd_key),
        },
        None => Row {
            kind: RowKind::Cwd,
            name: "current directory".into(),
            path: cwd.to_owned(),
            last_activity_ms: None,
            worktree: false,
            slug: String::new(),
            slugs: Vec::new(),
            matched: false,
        },
    };
    let mut rows = vec![cwd_row];
    rows.extend(
        matched
            .into_iter()
            .filter(|p| here.is_none() || project_key(&p.path) != cwd_key)
            .map(|p| Row {
                kind: RowKind::Project,
                name: p.name.clone(),
                path: p.path.clone(),
                last_activity_ms: p.last_activity_ms,
                worktree: p.worktree.is_some(),
                slug: p.slug.clone(),
                slugs: all_slugs(p),
                matched: false,
            }),
    );
    rows.push(Row {
        kind: RowKind::Path,
        name: "enter a path...".into(),
        path: String::new(),
        last_activity_ms: None,
        worktree: false,
        slug: String::new(),
        slugs: Vec::new(),
        matched: false,
    });
    rows
}

/// `Get-ProjectCount`: (known, hidden) for the title, from the rule that decides the rows. The
/// cwd's own project is listed whatever its flag and counted in neither hidden total.
pub(crate) fn project_count(projects: &[Project], filter: &str, cwd: &str) -> (usize, usize) {
    let listed = select_match(projects, filter, cwd);
    let cwd_key = project_key(cwd);
    let counted = |p: &Project| p.hidden && (cwd_key.is_empty() || project_key(&p.path) != cwd_key);
    let all = projects.iter().filter(|p| counted(p)).count();
    let shown = listed.iter().filter(|p| counted(p)).count();
    (listed.len(), all.saturating_sub(shown))
}

/// Every slug of the directory a path names, `[]` when it is no known project.
pub(crate) fn slugs_for_path(projects: &[Project], path: &str) -> Vec<String> {
    if path.is_empty() {
        return Vec::new();
    }
    let wanted = project_key(path);
    projects
        .iter()
        .filter(|p| project_key(&p.path) == wanted)
        .flat_map(all_slugs)
        .collect()
}

/// Where the start selection came from (the launch log's `projectSource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartSource {
    Cwd,
    Remembered,
    None,
}

impl StartSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cwd => "cwd",
            Self::Remembered => "remembered",
            Self::None => "none",
        }
    }
}

/// `Resolve-StartProject`: the cwd when it is a known project or holds a `.git`, else the
/// remembered project when it is still there (checked without a network wait), else none.
pub(crate) fn resolve_start(cwd: &str, remembered: &str, projects: &[Project]) -> (Option<String>, StartSource) {
    let c = project_key(cwd);
    if !c.is_empty() && (projects.iter().any(|p| project_key(&p.path) == c) || Path::new(cwd).join(".git").exists()) {
        return (Some(cwd.to_owned()), StartSource::Cwd);
    }
    if !remembered.is_empty() && path_present(remembered, &mut DriveCache::new()) {
        return (Some(remembered.to_owned()), StartSource::Remembered);
    }
    (None, StartSource::None)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::projects::registry::tests::Tree;

    pub(crate) fn proj(slug: &str, path: &str, name: &str) -> Project {
        Project {
            name: name.into(),
            path: path.into(),
            slug: slug.into(),
            slugs: vec![slug.into()],
            worktree: None,
            last_activity_ms: Some(0),
            hidden: false,
        }
    }

    fn two() -> Vec<Project> {
        vec![proj("A", r"C:\w\alpha", "alpha"), proj("B", r"C:\w\beta", "beta")]
    }

    #[test]
    fn filter_matches_name_and_path_literally() {
        let ps = two();
        assert_eq!(select_match(&ps, "al", "").len(), 1);
        assert_eq!(select_match(&ps, r"w\", "").len(), 2);
        assert_eq!(select_match(&ps, "", "").len(), 2);
        assert_eq!(select_match(&ps, r"C:\w\alpha", "").len(), 1, "a pasted path");
        assert_eq!(select_match(&ps, "[", "").len(), 0, "a bracket is literal and matches nothing");
    }

    #[test]
    fn start_selection_rules() {
        let ps = two();
        assert_eq!(
            resolve_start(r"C:\w\alpha", r"C:\w\beta", &ps),
            (Some(r"C:\w\alpha".into()), StartSource::Cwd)
        );
        assert_eq!(resolve_start("c:/w/alpha/", "", &ps).1, StartSource::Cwd, "any spelling of the cwd");
        let t = Tree::new("start");
        let remembered = t.dir("remembered");
        assert_eq!(
            resolve_start(r"C:\Users\someone\Desktop", &remembered, &ps).1,
            StartSource::Remembered
        );
        assert_eq!(resolve_start(r"C:\Users\someone\Desktop", "", &ps).1, StartSource::None);
        let vanished = t.0.join("vanished").to_string_lossy().into_owned();
        assert_eq!(resolve_start(r"C:\Users\someone\Desktop", &vanished, &ps).1, StartSource::None);
        assert_eq!(
            resolve_start("", r"\\claude-auto-no-such-host-604\share\repo", &[]).1,
            StartSource::Remembered
        );
    }

    #[test]
    fn hidden_rows_listed_only_by_filter_or_as_cwd() {
        let mut ps = vec![
            proj("real", r"C:\w\real", "real"),
            proj("intemp", r"C:\t\probe", "probe"),
            proj("job", r"C:\p\.claude\jobs\1\tmp\w585", "w585"),
            proj("linked", r"C:\w\linked", "linked"),
        ];
        ps[1].hidden = true;
        ps[2].hidden = true;
        let names = |v: Vec<&Project>| v.iter().map(|p| p.slug.clone()).collect::<Vec<_>>().join(",");
        assert_eq!(names(select_match(&ps, "", "")), "real,linked");
        assert_eq!(names(select_match(&ps, "w585", "")), "job");
        assert_eq!(names(select_match(&ps, "", r"C:\t\probe\")), "real,intemp,linked");
        let rows = project_rows(&ps, "", r"C:\p\.claude\jobs\1\tmp\w585");
        let kinds: Vec<RowKind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [RowKind::Cwd, RowKind::Project, RowKind::Project, RowKind::Path]);
        assert_eq!(rows[0].name, "w585 (current directory)");
        assert_eq!(rows[0].slug, "job");
        assert_eq!(project_count(&ps, "", ""), (2, 2));
        assert_eq!(project_count(&ps, "", r"C:\p\.claude\jobs\1\tmp\w585"), (3, 1));
        assert_eq!(project_count(&ps, "w585", ""), (1, 1));
    }

    #[test]
    fn a_filter_lists_visible_matches_ahead_of_hidden_ones() {
        let mut ps = vec![
            proj(
                "S",
                r"C:\Users\x\AppData\Local\Temp\claude\C--Projects-Acme\s1\scratchpad\cwd",
                "cwd",
            ),
            proj("A", r"C:\Projects\Acme", "Acme"),
        ];
        ps[0].hidden = true;
        let names: Vec<&str> = select_match(&ps, "acme", "").iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Acme", "cwd"]);
    }

    #[test]
    fn a_cwd_that_is_a_project_is_one_row() {
        let ps = two();
        let rows = project_rows(&ps, "", "c:/W/BETA/");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "beta (current directory)");
        assert_eq!(rows[0].slug, "B");
        assert_eq!(rows[1].name, "alpha");
        let plain = project_rows(&ps, "", r"C:\elsewhere");
        assert_eq!(plain.len(), 4);
        assert_eq!(plain[0].name, "current directory");
    }

    #[test]
    fn slugs_for_path_returns_every_slug() {
        let mut ps = two();
        ps[0].slugs = vec!["A".into(), "A2".into()];
        assert_eq!(slugs_for_path(&ps, "C:/W/ALPHA/"), ["A", "A2"]);
        assert_eq!(slugs_for_path(&ps, r"C:\nowhere"), [] as [&str; 0]);
    }
}
