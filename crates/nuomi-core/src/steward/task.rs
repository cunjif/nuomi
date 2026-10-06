//! Evolution task dependency graph + planner. (K-Steward-4, T4-4 ~ T4-6)

use std::collections::HashMap;

use crate::domain::{now_ms, DevRoleKind, EvolutionTask, StewardTaskStatus, TaskPhase};

/// A dependency graph over evolution tasks.
#[derive(Debug, Clone)]
pub struct TaskDependencyGraph {
    tasks: HashMap<String, EvolutionTask>,
    /// adjacency: task_id → its dependencies
    deps: HashMap<String, Vec<String>>,
}

impl TaskDependencyGraph {
    pub fn new(tasks: Vec<EvolutionTask>) -> Self {
        let mut task_map = HashMap::new();
        let mut dep_map = HashMap::new();
        for t in tasks {
            dep_map.insert(t.id.clone(), t.depends_on.clone());
            task_map.insert(t.id.clone(), t);
        }
        Self {
            tasks: task_map,
            deps: dep_map,
        }
    }

    /// Returns task ids whose status is `pending` and all dependencies are
    /// `completed`.
    pub fn ready_tasks(&self) -> Vec<String> {
        let mut ready = Vec::new();
        for (id, task) in &self.tasks {
            if task.status != StewardTaskStatus::Pending {
                continue;
            }
            let deps = self.deps.get(id).cloned().unwrap_or_default();
            let all_done = deps.iter().all(|dep_id| {
                self.tasks
                    .get(dep_id)
                    .map(|t| t.status == StewardTaskStatus::Completed)
                    .unwrap_or(false)
            });
            if all_done {
                ready.push(id.clone());
            }
        }
        ready
    }

    /// Topological sort (Kahn's algorithm). Returns Err if a cycle is detected.
    pub fn topological_sort(&self) -> Result<Vec<String>, String> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for id in self.tasks.keys() {
            in_degree.entry(id.clone()).or_insert(0);
            adj.entry(id.clone()).or_default();
        }
        for (id, deps) in &self.deps {
            for dep in deps {
                adj.entry(dep.clone()).or_default().push(id.clone());
                *in_degree.entry(id.clone()).or_insert(0) += 1;
            }
        }
        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(k, _)| k.clone())
            .collect();
        queue.sort();
        let mut result = Vec::new();
        while let Some(id) = queue.pop() {
            result.push(id.clone());
            if let Some(neighbors) = adj.get(&id) {
                for n in neighbors {
                    if let Some(d) = in_degree.get_mut(n) {
                        *d -= 1;
                        if *d == 0 {
                            queue.push(n.clone());
                            queue.sort();
                        }
                    }
                }
            }
        }
        if result.len() != self.tasks.len() {
            return Err("cycle detected in task dependency graph".into());
        }
        Ok(result)
    }

    pub fn get(&self, id: &str) -> Option<&EvolutionTask> {
        self.tasks.get(id)
    }

    pub fn all(&self) -> Vec<&EvolutionTask> {
        self.tasks.values().collect()
    }
}

/// Plans the 5-phase task breakdown for an evolution instruction.
pub struct TaskPlanner;

impl TaskPlanner {
    /// Creates 5 tasks (research → design → develop → test → verify) with
    /// linear dependencies. Each task has non-empty acceptance criteria.
    pub fn plan(cycle_id: &str, instruction: &str) -> Vec<EvolutionTask> {
        let now = now_ms();
        let phases = [
            (
                TaskPhase::Research,
                DevRoleKind::Researcher,
                "调研报告覆盖指令所述范围，含现状分析与改进方向",
            ),
            (
                TaskPhase::Design,
                DevRoleKind::Designer,
                "设计方案含架构变更点 + 接口契约 + 迁移策略",
            ),
            (
                TaskPhase::Develop,
                DevRoleKind::Developer,
                "实现代码通过 cargo check + clippy 零警告",
            ),
            (
                TaskPhase::Test,
                DevRoleKind::Tester,
                "测试全绿，覆盖新增逻辑分支与边界条件",
            ),
            (
                TaskPhase::Verify,
                DevRoleKind::Verifier,
                "验收门通过：产物 diff 可审、回滚计划完备",
            ),
        ];
        let mut tasks = Vec::with_capacity(5);
        let mut prev_id: Option<String> = None;
        for (i, (phase, role, criteria)) in phases.iter().enumerate() {
            let id = crate::domain::new_id();
            let depends_on = match &prev_id {
                Some(pid) => vec![pid.clone()],
                None => vec![],
            };
            let task = EvolutionTask {
                id: id.clone(),
                cycle_id: cycle_id.into(),
                phase: *phase,
                dev_role: *role,
                depends_on,
                status: StewardTaskStatus::Pending,
                acceptance_criteria: format!("[{}/5] {criteria}（指令: {instruction}）", i + 1),
                trigger_source: "steward".into(),
                created_at: now,
                updated_at: now,
            };
            prev_id = Some(id);
            tasks.push(task);
        }
        tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, deps: Vec<String>, status: StewardTaskStatus) -> EvolutionTask {
        EvolutionTask {
            id: id.into(),
            cycle_id: "c1".into(),
            phase: TaskPhase::Research,
            dev_role: DevRoleKind::Researcher,
            depends_on: deps,
            status,
            acceptance_criteria: "ac".into(),
            trigger_source: "test".into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn ready_tasks_respects_dependencies() {
        let tasks = vec![
            make_task("t1", vec![], StewardTaskStatus::Pending),
            make_task("t2", vec!["t1".into()], StewardTaskStatus::Pending),
            make_task("t3", vec!["t1".into()], StewardTaskStatus::Pending),
        ];
        let graph = TaskDependencyGraph::new(tasks);
        let ready = graph.ready_tasks();
        assert_eq!(ready, vec!["t1"]);
    }

    #[test]
    fn ready_tasks_advances_when_deps_complete() {
        let tasks = vec![
            make_task("t1", vec![], StewardTaskStatus::Completed),
            make_task("t2", vec!["t1".into()], StewardTaskStatus::Pending),
            make_task("t3", vec!["t1".into()], StewardTaskStatus::Running),
        ];
        let graph = TaskDependencyGraph::new(tasks);
        let ready = graph.ready_tasks();
        assert_eq!(ready, vec!["t2"]);
    }

    #[test]
    fn topological_sort_linear_chain() {
        let tasks = vec![
            make_task("t1", vec![], StewardTaskStatus::Pending),
            make_task("t2", vec!["t1".into()], StewardTaskStatus::Pending),
            make_task("t3", vec!["t2".into()], StewardTaskStatus::Pending),
        ];
        let graph = TaskDependencyGraph::new(tasks);
        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted, vec!["t1", "t2", "t3"]);
    }

    #[test]
    fn topological_sort_detects_cycle() {
        let tasks = vec![
            make_task("t1", vec!["t2".into()], StewardTaskStatus::Pending),
            make_task("t2", vec!["t1".into()], StewardTaskStatus::Pending),
        ];
        let graph = TaskDependencyGraph::new(tasks);
        assert!(graph.topological_sort().is_err());
    }

    #[test]
    fn planner_creates_5_linear_tasks() {
        let tasks = TaskPlanner::plan("c1", "improve error handling");
        assert_eq!(tasks.len(), 5);
        assert!(tasks[0].depends_on.is_empty());
        for i in 1..5 {
            assert_eq!(tasks[i].depends_on, vec![tasks[i - 1].id.clone()]);
        }
        for t in &tasks {
            assert!(!t.acceptance_criteria.is_empty());
        }
        assert_eq!(tasks[0].phase, TaskPhase::Research);
        assert_eq!(tasks[4].phase, TaskPhase::Verify);
    }
}
