//! Responses of the GitHub REST API: pages of issues, pull requests,
//! repositories, workflow runs, commits and releases.
//!
//! The payloads are the examples of the OpenAPI description of the API
//! (`github/rest-api-description`, vendored with
//! `scripts/update-benchmark-data.sh`).  Every example has a single item,
//! the pages repeat them 30 times (the default page size of the API) with
//! varied ids and optional data.  This is what API clients deserialize: a
//! lot of optional fields and nulls, the same nested user objects over and
//! over, enums given as strings and timestamps.
//!
//! The types are generated from the examples and the schemas of the
//! description (fields are optional where the schemas allow null).
use deser::{Deserialize, Serialize};

/// The number of items per page.
const PAGE_SIZE: u64 = 30;

/// The examples as vendored in `benchmark/data/github/examples.json`.
#[derive(Deserialize)]
struct Examples {
    #[deser(rename = "issue-items")]
    issues: Vec<Issue>,
    #[deser(rename = "pull-request-simple-items")]
    pull_requests: Vec<PullRequest>,
    #[deser(rename = "minimal-repository-items")]
    repositories: Vec<Repository>,
    #[deser(rename = "workflow-run-paginated")]
    workflow_runs: WorkflowRuns,
    #[deser(rename = "commit-items")]
    commits: Vec<Commit>,
    #[deser(rename = "release-items")]
    releases: Vec<Release>,
}

/// One page of every kind of response.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct Responses {
    issues: Vec<Issue>,
    pull_requests: Vec<PullRequest>,
    repositories: Vec<Repository>,
    workflow_runs: WorkflowRuns,
    commits: Vec<Commit>,
    releases: Vec<Release>,
}

/// Builds the pages from the vendored examples.
pub fn responses(examples: &str) -> Responses {
    let examples: Examples = deser_json::from_str(examples).expect("invalid GitHub examples");
    let issue = &examples.issues[0];
    let pull_request = &examples.pull_requests[0];
    let repository = &examples.repositories[0];
    let run = &examples.workflow_runs.workflow_runs[0];
    let commit = &examples.commits[0];
    let release = &examples.releases[0];
    Responses {
        issues: (0..PAGE_SIZE)
            .map(|i| {
                let mut issue = issue.clone();
                issue.id += i;
                issue.number += i;
                issue.title = format!("{} ({})", issue.title, i);
                // every third issue is closed, some have no body, labels
                // and assignees vary
                if i % 3 == 0 {
                    issue.state = State::Closed;
                    issue.closed_at = Some(issue.updated_at.clone());
                    issue.state_reason = Some(StateReason::Completed);
                } else {
                    issue.closed_by = None;
                    issue.state_reason = None;
                }
                if i % 4 == 0 {
                    issue.body = None;
                    issue.milestone = None;
                }
                if i % 2 == 0 {
                    issue.pull_request = None;
                }
                let label = issue.labels[0].clone();
                issue.labels = (0..i % 4).map(|_| label.clone()).collect();
                issue.assignees.truncate((i % 3) as usize);
                issue.assignee = issue.assignees.first().cloned();
                issue
            })
            .collect(),
        pull_requests: (0..PAGE_SIZE)
            .map(|i| {
                let mut pull_request = pull_request.clone();
                pull_request.id += i;
                pull_request.number += i;
                if i % 3 == 0 {
                    pull_request.state = State::Closed;
                    pull_request.closed_at = Some(pull_request.updated_at.clone());
                    pull_request.merged_at = Some(pull_request.updated_at.clone());
                }
                if i % 4 == 0 {
                    pull_request.body = None;
                    pull_request.milestone = None;
                }
                pull_request.draft = i % 5 == 0;
                pull_request
            })
            .collect(),
        repositories: (0..PAGE_SIZE)
            .map(|i| {
                let mut repository = repository.clone();
                repository.id += i;
                repository.name = format!("{}-{}", repository.name, i);
                if i % 2 == 0 {
                    repository.description = None;
                    repository.homepage = None;
                }
                repository
            })
            .collect(),
        workflow_runs: WorkflowRuns {
            total_count: PAGE_SIZE,
            workflow_runs: (0..PAGE_SIZE)
                .map(|i| {
                    let mut run = run.clone();
                    run.id += i;
                    run.run_number += i;
                    if i % 4 == 0 {
                        run.conclusion = None;
                        run.status = "in_progress".into();
                    }
                    run
                })
                .collect(),
        },
        commits: (0..PAGE_SIZE)
            .map(|i| {
                let mut commit = commit.clone();
                commit.sha = format!("{}{:02}", &commit.sha[..38], i);
                commit
            })
            .collect(),
        releases: (0..PAGE_SIZE)
            .map(|i| {
                let mut release = release.clone();
                release.id += i;
                release.tag_name = format!("v1.{}.0", i);
                if i % 3 == 0 {
                    release.body = None;
                    release.assets.clear();
                }
                release
            })
            .collect(),
    }
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Issue {
    pub id: u64,
    pub node_id: String,
    pub url: String,
    pub repository_url: String,
    pub labels_url: String,
    pub comments_url: String,
    pub events_url: String,
    pub html_url: String,
    pub number: u64,
    pub state: State,
    pub title: String,
    pub body: Option<String>,
    pub user: Option<SimpleUser>,
    pub labels: Vec<Label>,
    pub assignee: Option<SimpleUser>,
    pub assignees: Vec<SimpleUser>,
    pub milestone: Option<Milestone>,
    pub locked: bool,
    pub active_lock_reason: Option<String>,
    pub comments: u64,
    pub pull_request: Option<IssuePullRequest>,
    pub closed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_by: Option<SimpleUser>,
    pub author_association: AuthorAssociation,
    pub state_reason: Option<StateReason>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct SimpleUser {
    pub login: String,
    pub id: u64,
    pub node_id: String,
    pub avatar_url: String,
    pub gravatar_id: Option<String>,
    pub url: String,
    pub html_url: String,
    pub followers_url: String,
    pub following_url: String,
    pub gists_url: String,
    pub starred_url: String,
    pub subscriptions_url: String,
    pub organizations_url: String,
    pub repos_url: String,
    pub events_url: String,
    pub received_events_url: String,
    #[deser(rename = "type")]
    #[serde(rename = "type")]
    pub kind: UserType,
    pub site_admin: bool,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Label {
    pub id: u64,
    pub node_id: String,
    pub url: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub default: bool,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Milestone {
    pub url: String,
    pub html_url: String,
    pub labels_url: String,
    pub id: u64,
    pub node_id: String,
    pub number: u64,
    pub state: State,
    pub title: String,
    pub description: Option<String>,
    pub creator: Option<SimpleUser>,
    pub open_issues: u64,
    pub closed_issues: u64,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub due_on: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct IssuePullRequest {
    pub url: Option<String>,
    pub html_url: Option<String>,
    pub diff_url: Option<String>,
    pub patch_url: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct PullRequest {
    pub url: String,
    pub id: u64,
    pub node_id: String,
    pub html_url: String,
    pub diff_url: String,
    pub patch_url: String,
    pub issue_url: String,
    pub commits_url: String,
    pub review_comments_url: String,
    pub review_comment_url: String,
    pub comments_url: String,
    pub statuses_url: String,
    pub number: u64,
    pub state: State,
    pub locked: bool,
    pub title: String,
    pub user: Option<SimpleUser>,
    pub body: Option<String>,
    pub labels: Vec<Label>,
    pub milestone: Option<Milestone>,
    pub active_lock_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub merged_at: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub assignee: Option<SimpleUser>,
    pub assignees: Vec<SimpleUser>,
    pub requested_reviewers: Vec<SimpleUser>,
    pub requested_teams: Vec<RequestedTeam>,
    pub head: Head,
    pub base: Head,
    #[deser(rename = "_links")]
    #[serde(rename = "_links")]
    pub links: Links,
    pub author_association: AuthorAssociation,
    pub auto_merge: Option<AutoMerge>,
    pub draft: bool,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct RequestedTeam {
    pub id: u64,
    pub node_id: String,
    pub url: String,
    pub html_url: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub privacy: String,
    pub permission: String,
    pub members_url: String,
    pub repositories_url: String,
    pub parent: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Head {
    pub label: String,
    #[deser(rename = "ref")]
    #[serde(rename = "ref")]
    pub ref_: String,
    pub sha: String,
    pub user: Option<SimpleUser>,
    pub repo: NestedRepository,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct NestedRepository {
    pub id: u64,
    pub node_id: String,
    pub name: String,
    pub full_name: String,
    pub owner: SimpleUser,
    pub private: bool,
    pub html_url: String,
    pub description: Option<String>,
    pub fork: bool,
    pub url: String,
    pub archive_url: String,
    pub assignees_url: String,
    pub blobs_url: String,
    pub branches_url: String,
    pub collaborators_url: String,
    pub comments_url: String,
    pub commits_url: String,
    pub compare_url: String,
    pub contents_url: String,
    pub contributors_url: String,
    pub deployments_url: String,
    pub downloads_url: String,
    pub events_url: String,
    pub forks_url: String,
    pub git_commits_url: String,
    pub git_refs_url: String,
    pub git_tags_url: String,
    pub git_url: String,
    pub issue_comment_url: String,
    pub issue_events_url: String,
    pub issues_url: String,
    pub keys_url: String,
    pub labels_url: String,
    pub languages_url: String,
    pub merges_url: String,
    pub milestones_url: String,
    pub notifications_url: String,
    pub pulls_url: String,
    pub releases_url: String,
    pub ssh_url: String,
    pub stargazers_url: String,
    pub statuses_url: String,
    pub subscribers_url: String,
    pub subscription_url: String,
    pub tags_url: String,
    pub teams_url: String,
    pub trees_url: String,
    pub clone_url: String,
    pub mirror_url: Option<String>,
    pub hooks_url: String,
    pub svn_url: String,
    pub homepage: Option<String>,
    pub language: Option<String>,
    pub forks_count: u64,
    pub stargazers_count: u64,
    pub watchers_count: u64,
    pub size: u64,
    pub default_branch: String,
    pub open_issues_count: u64,
    pub is_template: bool,
    pub topics: Vec<String>,
    pub has_issues: bool,
    pub has_projects: bool,
    pub has_wiki: bool,
    pub has_pages: bool,
    pub has_downloads: bool,
    pub archived: bool,
    pub disabled: bool,
    pub visibility: String,
    pub pushed_at: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub permissions: Permissions,
    pub allow_rebase_merge: bool,
    pub template_repository: Option<String>,
    pub temp_clone_token: String,
    pub allow_squash_merge: bool,
    pub allow_auto_merge: bool,
    pub delete_branch_on_merge: bool,
    pub allow_merge_commit: bool,
    pub subscribers_count: u64,
    pub network_count: u64,
    pub license: Option<License>,
    pub forks: u64,
    pub open_issues: u64,
    pub watchers: u64,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Permissions {
    pub admin: bool,
    pub push: bool,
    pub pull: bool,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct License {
    pub key: String,
    pub name: String,
    pub url: Option<String>,
    pub spdx_id: Option<String>,
    pub node_id: String,
    pub html_url: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Links {
    #[deser(rename = "self")]
    #[serde(rename = "self")]
    pub self_: Link,
    pub html: Link,
    pub issue: Link,
    pub comments: Link,
    pub review_comments: Link,
    pub review_comment: Link,
    pub commits: Link,
    pub statuses: Link,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Link {
    pub href: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Repository {
    pub id: u64,
    pub node_id: String,
    pub name: String,
    pub full_name: String,
    pub owner: SimpleUser,
    pub private: bool,
    pub html_url: String,
    pub description: Option<String>,
    pub fork: bool,
    pub url: String,
    pub archive_url: String,
    pub assignees_url: String,
    pub blobs_url: String,
    pub branches_url: String,
    pub collaborators_url: String,
    pub comments_url: String,
    pub commits_url: String,
    pub compare_url: String,
    pub contents_url: String,
    pub contributors_url: String,
    pub deployments_url: String,
    pub downloads_url: String,
    pub events_url: String,
    pub forks_url: String,
    pub git_commits_url: String,
    pub git_refs_url: String,
    pub git_tags_url: String,
    pub git_url: String,
    pub issue_comment_url: String,
    pub issue_events_url: String,
    pub issues_url: String,
    pub keys_url: String,
    pub labels_url: String,
    pub languages_url: String,
    pub merges_url: String,
    pub milestones_url: String,
    pub notifications_url: String,
    pub pulls_url: String,
    pub releases_url: String,
    pub ssh_url: String,
    pub stargazers_url: String,
    pub statuses_url: String,
    pub subscribers_url: String,
    pub subscription_url: String,
    pub tags_url: String,
    pub teams_url: String,
    pub trees_url: String,
    pub clone_url: String,
    pub mirror_url: Option<String>,
    pub hooks_url: String,
    pub svn_url: String,
    pub homepage: Option<String>,
    pub language: Option<String>,
    pub forks_count: u64,
    pub stargazers_count: u64,
    pub watchers_count: u64,
    pub size: u64,
    pub default_branch: String,
    pub open_issues_count: u64,
    pub is_template: bool,
    pub topics: Vec<String>,
    pub has_issues: bool,
    pub has_projects: bool,
    pub has_wiki: bool,
    pub has_pages: bool,
    pub has_downloads: bool,
    pub archived: bool,
    pub disabled: bool,
    pub visibility: String,
    pub pushed_at: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub permissions: Permissions,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct WorkflowRuns {
    pub total_count: u64,
    pub workflow_runs: Vec<WorkflowRun>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct WorkflowRun {
    pub id: u64,
    pub name: String,
    pub node_id: String,
    pub check_suite_id: u64,
    pub check_suite_node_id: String,
    pub head_branch: String,
    pub head_sha: String,
    pub run_number: u64,
    pub event: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub workflow_id: u64,
    pub url: String,
    pub html_url: String,
    pub pull_requests: Vec<PullRequestMinimal>,
    pub created_at: String,
    pub updated_at: String,
    pub actor: SimpleUser,
    pub run_attempt: u64,
    pub run_started_at: String,
    pub triggering_actor: SimpleUser,
    pub jobs_url: String,
    pub logs_url: String,
    pub check_suite_url: String,
    pub artifacts_url: String,
    pub cancel_url: String,
    pub rerun_url: String,
    pub workflow_url: String,
    pub head_commit: HeadCommit,
    pub repository: NestedRepository2,
    pub head_repository: NestedRepository3,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct HeadCommit {
    pub id: String,
    pub tree_id: String,
    pub message: String,
    pub timestamp: String,
    pub author: Author,
    pub committer: Author,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Author {
    pub name: String,
    pub email: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct NestedRepository2 {
    pub id: u64,
    pub node_id: String,
    pub name: String,
    pub full_name: String,
    pub owner: SimpleUser,
    pub private: bool,
    pub html_url: String,
    pub description: Option<String>,
    pub fork: bool,
    pub url: String,
    pub archive_url: String,
    pub assignees_url: String,
    pub blobs_url: String,
    pub branches_url: String,
    pub collaborators_url: String,
    pub comments_url: String,
    pub commits_url: String,
    pub compare_url: String,
    pub contents_url: String,
    pub contributors_url: String,
    pub deployments_url: String,
    pub downloads_url: String,
    pub events_url: String,
    pub forks_url: String,
    pub git_commits_url: String,
    pub git_refs_url: String,
    pub git_tags_url: String,
    pub git_url: String,
    pub issue_comment_url: String,
    pub issue_events_url: String,
    pub issues_url: String,
    pub keys_url: String,
    pub labels_url: String,
    pub languages_url: String,
    pub merges_url: String,
    pub milestones_url: String,
    pub notifications_url: String,
    pub pulls_url: String,
    pub releases_url: String,
    pub ssh_url: String,
    pub stargazers_url: String,
    pub statuses_url: String,
    pub subscribers_url: String,
    pub subscription_url: String,
    pub tags_url: String,
    pub teams_url: String,
    pub trees_url: String,
    pub hooks_url: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct NestedRepository3 {
    pub id: u64,
    pub node_id: String,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub owner: SimpleUser,
    pub html_url: String,
    pub description: Option<String>,
    pub fork: bool,
    pub url: String,
    pub forks_url: String,
    pub keys_url: String,
    pub collaborators_url: String,
    pub teams_url: String,
    pub hooks_url: String,
    pub issue_events_url: String,
    pub events_url: String,
    pub assignees_url: String,
    pub branches_url: String,
    pub tags_url: String,
    pub blobs_url: String,
    pub git_tags_url: String,
    pub git_refs_url: String,
    pub trees_url: String,
    pub statuses_url: String,
    pub languages_url: String,
    pub stargazers_url: String,
    pub contributors_url: String,
    pub subscribers_url: String,
    pub subscription_url: String,
    pub commits_url: String,
    pub git_commits_url: String,
    pub comments_url: String,
    pub issue_comment_url: String,
    pub contents_url: String,
    pub compare_url: String,
    pub merges_url: String,
    pub archive_url: String,
    pub downloads_url: String,
    pub issues_url: String,
    pub pulls_url: String,
    pub milestones_url: String,
    pub notifications_url: String,
    pub labels_url: String,
    pub releases_url: String,
    pub deployments_url: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Commit {
    pub url: String,
    pub sha: String,
    pub node_id: String,
    pub html_url: String,
    pub comments_url: String,
    pub commit: NestedCommit,
    pub author: Option<SimpleUser>,
    pub committer: Option<SimpleUser>,
    pub parents: Vec<Tree>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct NestedCommit {
    pub url: String,
    pub author: Option<Author2>,
    pub committer: Option<Author2>,
    pub message: String,
    pub tree: Tree,
    pub comment_count: u64,
    pub verification: Verification,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Author2 {
    pub name: String,
    pub email: String,
    pub date: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Tree {
    pub url: String,
    pub sha: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Verification {
    pub verified: bool,
    pub reason: String,
    pub signature: Option<String>,
    pub payload: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Release {
    pub url: String,
    pub html_url: String,
    pub assets_url: String,
    pub upload_url: String,
    pub tarball_url: Option<String>,
    pub zipball_url: Option<String>,
    pub id: u64,
    pub node_id: String,
    pub tag_name: String,
    pub target_commitish: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    pub published_at: Option<String>,
    pub author: SimpleUser,
    pub assets: Vec<Asset>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct Asset {
    pub url: String,
    pub browser_download_url: String,
    pub id: u64,
    pub node_id: String,
    pub name: String,
    pub label: Option<String>,
    pub state: String,
    pub content_type: String,
    pub size: u64,
    pub download_count: u64,
    pub created_at: String,
    pub updated_at: String,
    pub uploader: Option<SimpleUser>,
}

#[derive(
    Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone, Copy,
)]
pub enum State {
    #[deser(rename = "open")]
    #[serde(rename = "open")]
    Open,
    #[deser(rename = "closed")]
    #[serde(rename = "closed")]
    Closed,
}

#[derive(
    Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone, Copy,
)]
pub enum AuthorAssociation {
    #[deser(rename = "COLLABORATOR")]
    #[serde(rename = "COLLABORATOR")]
    Collaborator,
    #[deser(rename = "CONTRIBUTOR")]
    #[serde(rename = "CONTRIBUTOR")]
    Contributor,
    #[deser(rename = "FIRST_TIMER")]
    #[serde(rename = "FIRST_TIMER")]
    FirstTimer,
    #[deser(rename = "FIRST_TIME_CONTRIBUTOR")]
    #[serde(rename = "FIRST_TIME_CONTRIBUTOR")]
    FirstTimeContributor,
    #[deser(rename = "MANNEQUIN")]
    #[serde(rename = "MANNEQUIN")]
    Mannequin,
    #[deser(rename = "MEMBER")]
    #[serde(rename = "MEMBER")]
    Member,
    #[deser(rename = "NONE")]
    #[serde(rename = "NONE")]
    None,
    #[deser(rename = "OWNER")]
    #[serde(rename = "OWNER")]
    Owner,
}

#[derive(
    Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone, Copy,
)]
pub enum StateReason {
    #[deser(rename = "completed")]
    #[serde(rename = "completed")]
    Completed,
    #[deser(rename = "reopened")]
    #[serde(rename = "reopened")]
    Reopened,
    #[deser(rename = "not_planned")]
    #[serde(rename = "not_planned")]
    NotPlanned,
    #[deser(rename = "duplicate")]
    #[serde(rename = "duplicate")]
    Duplicate,
}

#[derive(
    Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone, Copy,
)]
pub enum UserType {
    #[deser(rename = "User")]
    #[serde(rename = "User")]
    User,
    #[deser(rename = "Organization")]
    #[serde(rename = "Organization")]
    Organization,
    #[deser(rename = "Bot")]
    #[serde(rename = "Bot")]
    Bot,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct AutoMerge {
    pub enabled_by: SimpleUser,
    pub merge_method: MergeMethod,
    pub commit_title: String,
    pub commit_message: String,
}

#[derive(
    Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone, Copy,
)]
pub enum MergeMethod {
    #[deser(rename = "merge")]
    #[serde(rename = "merge")]
    Merge,
    #[deser(rename = "squash")]
    #[serde(rename = "squash")]
    Squash,
    #[deser(rename = "rebase")]
    #[serde(rename = "rebase")]
    Rebase,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct PullRequestMinimal {
    pub id: u64,
    pub number: u64,
    pub url: String,
}
