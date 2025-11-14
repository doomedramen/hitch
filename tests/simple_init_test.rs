use anyhow::Result;

mod common;
use common::{TestEnvironment, TestRunner};

#[test]
fn test_basic_init() -> Result<()> {
    let env = TestEnvironment::new()?;
    let runner = TestRunner::new();

    runner.test("Basic init creates hitch-metadata branch", || {
        let output = env.run_hitch(&["init"])?;
        runner.assert_contains_simple(&output, "✅ Hitch initialized successfully")?;

        Ok(())
    })?;

    Ok(())
}