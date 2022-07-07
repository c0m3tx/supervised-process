use std::{
    process::{Child, Command},
    thread,
    time::Duration,
};

pub type SupervisorTest = Box<dyn FnMut(&mut Child) -> bool>;

pub struct SupervisedProcess {
    process: String,
    args: Vec<String>,
    restart_times: Option<u64>,
    check_interval: Duration,
    backoff_time: Duration,
    tests: Vec<(String, SupervisorTest)>,
}

impl Default for SupervisedProcess {
    fn default() -> Self {
        Self {
            process: "".to_string(),
            args: vec![],
            restart_times: None,
            check_interval: Duration::from_secs(30),
            backoff_time: Duration::from_secs(30),
            tests: vec![],
        }
    }
}

impl SupervisedProcess {
    pub fn new(process: String) -> Self {
        Self {
            process,
            ..Self::default()
        }
    }

    pub fn with_check_interval(self, check_interval: Duration) -> Self {
        Self {
            check_interval,
            ..self
        }
    }

    pub fn with_backoff_time(self, backoff_time: Duration) -> Self {
        Self {
            backoff_time,
            ..self
        }
    }

    pub fn with_restart_times(self, restart_times: u64) -> Self {
        Self {
            restart_times: Some(restart_times),
            ..self
        }
    }

    pub fn with_args(self, args: impl IntoIterator<Item = impl ToString>) -> Self {
        let args = args.into_iter().map(|a| a.to_string()).collect();
        Self { args, ..self }
    }

    pub fn add_test(self, name: &str, test: SupervisorTest) -> Self {
        let mut tests = self.tests;
        tests.push((name.into(), test));

        Self { tests, ..self }
    }

    pub fn should_restart(&mut self) -> bool {
        match self.restart_times {
            None => true,
            Some(0) => false,
            Some(times) => {
                self.restart_times = Some(times - 1);
                true
            }
        }
    }

    pub fn run(&mut self) {
        let process = Command::new(self.process.clone())
            .args(self.args.clone())
            .spawn();
        let mut child = process.expect("Failed to start process");
        loop {
            thread::sleep(self.check_interval);
            println!("Running tests on {}", self.process);
            if self.tests.iter_mut().any(|test| {
                if !test.1(&mut child) {
                    println!("{} test failed", test.0);
                    true
                } else {
                    false
                }
            }) {
                child.kill().unwrap_or_else(|_e| {
                    println!("Failed to kill {}", self.process);
                });

                if self.should_restart() {
                    thread::sleep(self.backoff_time);
                    println!("Restarting {}", self.process);

                    return self.run();
                } else {
                    println!("Will not restart {}", self.process);
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds_a_process_with_check_interval() {
        let process =
            SupervisedProcess::new("test".to_string()).with_check_interval(Duration::from_secs(15));
        assert_eq!(process.check_interval, Duration::from_secs(15));
    }

    #[test]
    fn it_builds_a_process_with_backoff_time() {
        let process =
            SupervisedProcess::new("test".to_string()).with_backoff_time(Duration::from_secs(15));
        assert_eq!(process.backoff_time, Duration::from_secs(15));
    }

    #[test]
    fn it_builds_a_process_adding_a_test() {
        let process = SupervisedProcess::new("test".to_string())
            .add_test("always false", Box::from(|_child: &mut Child| false));
        assert_eq!(process.tests.len(), 1);
    }

    #[test]
    fn use_child_in_test() {
        let mut process = SupervisedProcess::new("sleep".to_string())
            .with_args(vec!["0.2"])
            .add_test(
                "still running",
                Box::from(|child: &mut Child| match child.try_wait() {
                    Ok(None) => true,
                    Ok(Some(exit_value)) => {
                        println!("Got exit value {}", exit_value);
                        false
                    }
                    _ => false,
                }),
            )
            .with_check_interval(Duration::from_millis(5))
            .with_restart_times(0);
        process.run();
    }

    #[test]
    fn it_runs_the_command() {
        let mut process = SupervisedProcess::new("ls".to_string())
            .add_test("always false", Box::from(|_child: &mut Child| false))
            .with_check_interval(Duration::from_millis(10))
            .with_backoff_time(Duration::from_millis(10))
            .with_restart_times(1);
        process.run();
    }

    #[test]
    fn it_runs_the_command_with_args() {
        let mut process = SupervisedProcess::new("ls".to_string())
            .with_args(vec!["-l"])
            .add_test("always false", Box::from(|_child: &mut Child| false))
            .with_check_interval(Duration::from_millis(10))
            .with_backoff_time(Duration::from_millis(10))
            .with_restart_times(1);
        process.run();
    }
}
