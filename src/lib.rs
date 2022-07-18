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
    on_test_start: Option<&'a dyn Fn()>,
    on_tests_passing: Option<&'a dyn Fn()>,
    on_test_ok: Option<&'a dyn Fn(&str)>,
    on_test_error: Option<&'a dyn Fn(&str)>,
    on_restart: Option<&'a dyn Fn()>,
    on_no_restart: Option<&'a dyn Fn()>,
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
            on_tests_passing: None,
            on_test_ok: None,
            on_test_error: None,
            on_restart: None,
            on_no_restart: None,
        }
    }
}

macro_rules! event {
    ($handler:expr) => {
        if let Some(handler) = $handler {
            handler();
        }
    };

    ($handler:expr, $($arg:expr),+) => {
        if let Some(handler) = $handler {
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

    pub fn on_restart(self, on_restart: &'a dyn Fn()) -> Self {
        Self {
            on_restart: Some(on_restart),
            ..self
        }
    }

    pub fn on_no_restart(self, on_no_restart: &'a dyn Fn()) -> Self {
        Self {
            on_no_restart: Some(on_no_restart),
            ..self
        }
    }

    pub fn on_test_start(self, on_test_start: &'a dyn Fn()) -> Self {
        Self {
            on_test_start: Some(on_test_start),
            ..self
        }
    }

    pub fn on_tests_passing(self, on_tests_passing: &'a dyn Fn()) -> Self {
        Self {
            on_tests_passing: Some(on_tests_passing),
            ..self
        }
    }

    pub fn on_test_ok(self, on_test_ok: &'a dyn Fn(&str)) -> Self {
        Self {
            on_test_ok: Some(on_test_ok),
            ..self
        }
    }

    pub fn on_test_error(self, on_test_error: &'a dyn Fn(&str)) -> Self {
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
                    event!(self.on_test_ok, test.0.as_str());
                    true
                } else {
                    event!(self.on_test_error, &test.0);
                    false
                }
            }) {
                let _ = child.kill();

                if self.should_restart() {
                    thread::sleep(self.backoff_time);
                    event!(self.on_restart);
                    return Ok(Operation::Restart);
                } else {
                    event!(self.on_no_restart);
                    return Ok(Operation::NoRestart);
                }
            } else {
                event!(self.on_tests_passing);
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
                Ok(Operation::Restart) => continue,
                Ok(Operation::NoRestart) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

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
    fn event_on_restart() {
        let restart_count: RefCell<i32> = RefCell::new(0);
        let restart_fn = || {
            (*restart_count.borrow_mut()) += 1;
        };

        let mut process = SupervisedProcess::new("echo".to_string())
            .with_args(vec!["-n"])
            .add_test("always false", Box::from(|_: &mut Child| false))
            .with_check_interval(Duration::from_millis(1))
            .with_backoff_time(Duration::from_millis(1))
            .with_restart_times(1)
            .on_restart(&restart_fn);

        assert!(process.run().is_ok());
        let guard = restart_count.borrow();

        assert_eq!(*guard, 1)
    }

    #[test]
    fn event_on_no_restart() {
        let no_restart_count: RefCell<i32> = RefCell::new(0);
        let no_restart_fn = || {
            (*no_restart_count.borrow_mut()) += 1;
        };

        let mut process = SupervisedProcess::new("echo".to_string())
            .with_args(vec!["-n"])
            .add_test("always false", Box::from(|_: &mut Child| false))
            .with_check_interval(Duration::from_millis(1))
            .with_backoff_time(Duration::from_millis(1))
            .with_restart_times(1)
            .on_no_restart(&no_restart_fn);

        assert!(process.run().is_ok());

        assert_eq!(*no_restart_count.borrow(), 1);
    }

    #[test]
    fn event_on_test_error() {
        let error_fn = |name: &str| assert_eq!("always false", name);

        let mut process = SupervisedProcess::new("echo".to_string())
            .with_args(vec!["-n"])
            .add_test("always false", Box::from(|_: &mut Child| false))
            .with_check_interval(Duration::from_millis(1))
            .with_backoff_time(Duration::from_millis(1))
            .with_restart_times(0)
            .on_test_error(&error_fn);

        assert!(process.run().is_ok());
    }

    #[test]
    fn event_on_test_run() {
        let test_run_count: RefCell<i32> = RefCell::new(0);
        let test_run_fn = || {
            (*test_run_count.borrow_mut()) += 1;
        };

        let mut process = SupervisedProcess::new("echo".to_string())
            .with_args(vec!["-n"])
            .add_test("always false", Box::from(|_: &mut Child| false))
            .with_check_interval(Duration::from_millis(1))
            .with_backoff_time(Duration::from_millis(1))
            .with_restart_times(1)
            .on_test_start(&test_run_fn);

        assert!(process.run().is_ok());
        assert_eq!(*test_run_count.borrow(), 2);
    }

    #[test]
    fn event_on_test_ok() {
        let mut test_ok_count = 0;
        let test_ok_ptr: *mut i32 = &mut test_ok_count;

        let test_ok_fn = || unsafe { *test_ok_ptr += 1 };

        let mut process = SupervisedProcess::new("sleep".to_string())
            .with_args(vec!["0.1"])
            .add_test(
                "not running",
                Box::from(|child: &mut Child| {
                    if let Ok(None) = child.try_wait() {
                        true
                    } else {
                        false
                    }
                }),
            )
            .with_check_interval(Duration::from_millis(80))
            .with_backoff_time(Duration::from_millis(80))
            .with_restart_times(0)
            .on_tests_passing(&test_ok_fn);
        assert!(process.run().is_ok());
        drop(process);

        assert_eq!(test_ok_count, 1);
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
        let mut process = SupervisedProcess::new("echo".to_string())
            .add_test("always false", Box::from(|_child: &mut Child| false))
            .with_check_interval(Duration::from_millis(10))
            .with_backoff_time(Duration::from_millis(10))
            .with_restart_times(1);
        assert!(process.run().is_ok());
    }

    #[test]
    fn it_runs_the_command_with_args() {
        let mut process = SupervisedProcess::new("echo".to_string())
            .with_args(vec!["-n"])
            .add_test("always false", Box::from(|_child: &mut Child| false))
            .with_check_interval(Duration::from_millis(10))
            .with_backoff_time(Duration::from_millis(10))
            .with_restart_times(1);
        assert!(process.run().is_ok());
    }
}
