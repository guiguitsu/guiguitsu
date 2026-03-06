pub struct CommitInfo {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub author: String,
    pub timestamp: String,
}

pub struct StackInfo {
    pub name: String,
    pub commits: Vec<CommitInfo>,
}

pub trait StackProvider {
    fn get_stacks(&self) -> Vec<StackInfo>;
}

pub struct DummyStackProvider;

impl StackProvider for DummyStackProvider {
    fn get_stacks(&self) -> Vec<StackInfo> {
        vec![
            StackInfo {
                name: "feature/auth-redesign".into(),
                commits: vec![
                    CommitInfo {
                        change_id: "a1b2c3d4".into(),
                        commit_id: "ff001122".into(),
                        description: "Add OAuth2 login flow".into(),
                        author: "Alice".into(),
                        timestamp: "2025-06-01 10:30".into(),
                    },
                    CommitInfo {
                        change_id: "e5f6a7b8".into(),
                        commit_id: "ff334455".into(),
                        description: "Refactor session handling".into(),
                        author: "Alice".into(),
                        timestamp: "2025-05-30 14:20".into(),
                    },
                    CommitInfo {
                        change_id: "c9d0e1f2".into(),
                        commit_id: "ff667788".into(),
                        description: "Add user profile endpoint".into(),
                        author: "Bob".into(),
                        timestamp: "2025-05-29 09:15".into(),
                    },
                ],
            },
            StackInfo {
                name: "fix/memory-leak".into(),
                commits: vec![
                    CommitInfo {
                        change_id: "11223344".into(),
                        commit_id: "aa001122".into(),
                        description: "Fix connection pool leak".into(),
                        author: "Carol".into(),
                        timestamp: "2025-06-02 16:45".into(),
                    },
                    CommitInfo {
                        change_id: "55667788".into(),
                        commit_id: "aa334455".into(),
                        description: "Add leak detection test".into(),
                        author: "Carol".into(),
                        timestamp: "2025-06-02 15:30".into(),
                    },
                ],
            },
            StackInfo {
                name: "chore/deps-update".into(),
                commits: vec![
                    CommitInfo {
                        change_id: "aabbccdd".into(),
                        commit_id: "bb001122".into(),
                        description: "Bump dependencies to latest".into(),
                        author: "Dave".into(),
                        timestamp: "2025-06-03 08:00".into(),
                    },
                ],
            },
            StackInfo {
                name: "main".into(),
                commits: vec![
                    CommitInfo {
                        change_id: "00112233".into(),
                        commit_id: "cc001122".into(),
                        description: "Release v1.2.0".into(),
                        author: "Alice".into(),
                        timestamp: "2025-05-28 12:00".into(),
                    },
                    CommitInfo {
                        change_id: "44556677".into(),
                        commit_id: "cc334455".into(),
                        description: "Merge fix/timeout-handling".into(),
                        author: "Bob".into(),
                        timestamp: "2025-05-27 18:30".into(),
                    },
                    CommitInfo {
                        change_id: "8899aabb".into(),
                        commit_id: "cc667788".into(),
                        description: "Update CI pipeline config".into(),
                        author: "Carol".into(),
                        timestamp: "2025-05-26 11:00".into(),
                    },
                    CommitInfo {
                        change_id: "ccddeeff".into(),
                        commit_id: "cc99aabb".into(),
                        description: "Initial project setup".into(),
                        author: "Dave".into(),
                        timestamp: "2025-05-25 09:00".into(),
                    },
                    CommitInfo {
                        change_id: "dd112233".into(),
                        commit_id: "dd001122".into(),
                        description: "Add README and contributing guide".into(),
                        author: "Alice".into(),
                        timestamp: "2025-05-24 16:00".into(),
                    },
                    CommitInfo {
                        change_id: "dd445566".into(),
                        commit_id: "dd334455".into(),
                        description: "Set up logging framework".into(),
                        author: "Bob".into(),
                        timestamp: "2025-05-24 11:30".into(),
                    },
                    CommitInfo {
                        change_id: "dd778899".into(),
                        commit_id: "dd667788".into(),
                        description: "Configure database migrations".into(),
                        author: "Carol".into(),
                        timestamp: "2025-05-23 14:00".into(),
                    },
                    CommitInfo {
                        change_id: "ddaabbcc".into(),
                        commit_id: "dd99aabb".into(),
                        description: "Add health check endpoint".into(),
                        author: "Dave".into(),
                        timestamp: "2025-05-23 10:15".into(),
                    },
                    CommitInfo {
                        change_id: "ee112233".into(),
                        commit_id: "ee001122".into(),
                        description: "Implement rate limiting middleware".into(),
                        author: "Alice".into(),
                        timestamp: "2025-05-22 17:45".into(),
                    },
                    CommitInfo {
                        change_id: "ee445566".into(),
                        commit_id: "ee334455".into(),
                        description: "Add integration test suite".into(),
                        author: "Bob".into(),
                        timestamp: "2025-05-22 09:00".into(),
                    },
                    CommitInfo {
                        change_id: "ee778899".into(),
                        commit_id: "ee667788".into(),
                        description: "Fix CORS configuration".into(),
                        author: "Carol".into(),
                        timestamp: "2025-05-21 13:30".into(),
                    },
                    CommitInfo {
                        change_id: "eeaabbcc".into(),
                        commit_id: "ee99aabb".into(),
                        description: "Scaffold CLI argument parsing".into(),
                        author: "Dave".into(),
                        timestamp: "2025-05-21 08:00".into(),
                    },
                ],
            },
        ]
    }
}
