//! 离线自检套件必须全绿；失败时打印每个用例细节。

use parallaxe_core::selftest;
use parallaxe_core::targets::Registry;

#[tokio::test]
async fn offline_suite_passes() {
    let registry = Registry::embedded().expect("内嵌资产应通过校验");
    let outcome = selftest::run(&registry, None, None)
        .await
        .expect("selftest 运行失败");
    for case in &outcome.cases {
        println!(
            "[{}] {} — {}",
            if case.ok { "ok" } else { "FAIL" },
            case.name,
            case.detail
        );
    }
    assert_eq!(outcome.failed, 0, "离线用例存在失败项");
    assert!(outcome.passed >= 15, "用例数量异常");
    assert_eq!(outcome.exit_code(), 0);
}
