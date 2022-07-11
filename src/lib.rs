use std::{
    process::{Child, Command},
    thread,
    time::Duration,
};

enum Operation {
    Restart,
    NoRestart,
}

pub type SupervisorTest = Box<dyn FnMut(&mut Child) -> bool>;

pub struct SupervisedProcess<'a> {
    process: String,
    args: Vec<String>,
    restart_times: Option<u64>,
    check_interval: Duration,
    backoff_time: Duration,
    tests: Vec<(String, SupervisorTest)>,
    on_test_start: Option<&'a mut dyn FnMut()>,
    on_tests_ok: Option<&'a mut dyn FnMut()>,
    on_test_error: Option<&'a mut dyn FnMut(&str)>,
    on_restart: Option<&'a mut dyn FnMut()>,
}

impl<'a> Default for SupervisedProcess<'a> {
    fn default() -> Self {
        Self {
            process: "".to_string(),
            args: vec![],
            restart_times: None,
            check_interval: Duration::from_secs(30),
            backoff_time: Duration::from_secs(30),
            tests: vec![],
            on_test_start: None,
            on_tests_ok: None,
            on_test_error: None,
            on_restart: None,
        }
    }
}

macro_rules! event {
    ($handler:expr) => {
        if let Some(handler) = &mut $handler {
            handler();
        }
    };

    ($handler:expr, $($arg:expr),+) => {
        if let Some(handler) = &mut $handler {
            handler($($arg),+);
        }
    };
}

impl<'a> SupervisedProcess<'a> {
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

    pub fn on_restart(self, on_restart: &'a mut dyn FnMut()) -> Self {
        Self {
            on_restart: Some(on_restart),
            ..self
        }
    }

    pub fn on_test_start(self, on_test_start: &'a mut dyn FnMut()) -> Self {
        Self {
            on_test_start: Some(on_test_start),
            ..self
        }
    }

    pub fn on_tests_ok(self, on_tests_ok: &'a mut dyn FnMut()) -> Self {
        Self {
            on_tests_ok: Some(on_tests_ok),
            ..self
        }
    }

    pub fn on_test_error(self, on_test_error: &'a mut dyn FnMut(&str)) -> Self {
        Self {
            on_test_error: Some(on_test_error),
            ..self
        }
    }

    fn test_loop(&mut self, child: &mut Child) -> Result<Operation, String> {
        loop {
            thread::sleep(self.check_interval);

            event!(self.on_test_start);

            if !self.tests.iter_mut().all(|test| {
                if test.1(child) {
                    return true;
                }

                event!(self.on_test_error, &test.0);
                false
            }) {
                let _ = child.kill();

                if self.should_restart() {
                    thread::sleep(self.backoff_time);
                    event!(self.on_restart);

                    return Ok(Operation::Restart);
                } else {
                    // println!("Will not restart {}", self.process);
                    return Ok(Operation::NoRestart);
                }
            } else {
                event!(self.on_tests_ok);
            }
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        loop {
            let process = Command::new(self.process.clone())
                .args(self.args.clone())
                .spawn();
            let mut child = process.map_err(|_| String::from("Failed to start process"))?;
            match self.test_loop(&mut child) {
                Ok(Operation::NoRestart) => return Ok(()),
                Err(e) => return Err(e),
                _ => {}
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
    fn it_restarts_once() {
        let mut restart_count: i32 = 0;
        let mut restart_fn = || {
            restart_count += 1;
        };

        let mut start_fn = || {
            println!("Started");
        };

        let mut process = SupervisedProcess::new("ls".to_string())
            .add_test("always false", Box::from(|_: &mut Child| false))
            .with_check_interval(Duration::from_millis(1))
            .with_backoff_time(Duration::from_millis(1))
            .with_restart_times(1)
            .on_restart(&mut restart_fn)
            .on_test_start(&mut start_fn);

        assert!(process.run().is_ok());
        assert_eq!(restart_count, 1)
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
        assert!(process.run().is_ok());
    }

    #[test]
    fn it_runs_the_command() {
        let mut process = SupervisedProcess::new("ls".to_string())
            .add_test("always false", Box::from(|_child: &mut Child| false))
            .with_check_interval(Duration::from_millis(10))
            .with_backoff_time(Duration::from_millis(10))
            .with_restart_times(1);
        assert!(process.run().is_ok());
    }

    #[test]
    fn it_runs_the_command_with_args() {
        let mut process = SupervisedProcess::new("ls".to_string())
            .with_args(vec!["-l"])
            .add_test("always false", Box::from(|_child: &mut Child| false))
            .with_check_interval(Duration::from_millis(10))
            .with_backoff_time(Duration::from_millis(10))
            .with_restart_times(1);
        assert!(process.run().is_ok());
    }
}
