# Android 打包说明（GameActivity 入口，已真机验证）

> 状态：2026-11-23 在 MuMu 12 #0（Android 15 / SDK 35 / x86_64+arm64 abi list）上完成
> `x86_64` 与 `arm64-v8a` 两个变体的安装、启动、进程存活与权限核验。

## 1. 入口契约（必须与 cdylib 匹配）

`crates/gui` 使用 `android-activity` + `winit` 的 **game-activity** 模式，编出的
`libparallaxe_gui.so` 导出：

- `GameActivity_onCreate`
- `Java_com_google_androidgamesdk_GameActivity_initializeNativeCode`
- `android_main`

**没有** `ANativeActivity_onCreate`。因此 `AndroidManifest.xml` 必须声明
`com.google.androidgamesdk.GameActivity`，并且**不要**再声明
`android.app.NativeActivity`。

反例（2026-11-23 MuMu 实测）：
```
E AndroidRuntime: java.lang.UnsatisfiedLinkError: Unable to load native library
  ".../lib/x86_64/libparallaxe_gui.so": undefined symbol: ANativeActivity_onCreate
```
Activity 类型与 `.so` 实现不匹配时，App 安装成功但启动即退（`onAppLaunch` 后立刻
`onAppClose`）。`cargo ndk build/check` 只能证明“能编译”，不能证明“能启动”。

## 2. Java 层依赖

`GameActivity` 继承 `androidx.appcompat.app.AppCompatActivity`，需要：

```kotlin
implementation("androidx.games:games-activity:4.4.0")
implementation("androidx.appcompat:appcompat:1.7.0")
```

- **AppCompat 必须显式声明**：`games-activity:4.4.0` 的 POM 没有声明任何依赖，
  只加 games-activity 会在运行期 `ClassNotFoundException: androidx.appcompat.app.AppCompatActivity`。
- Manifest 需要 AppCompat 主题（当前用 `@style/Theme.AppCompat.NoActionBar`）。
- **不要启用 prefab / 上游 C++ glue**：`android-activity` 自带 Rust glue，混用会冲突。

## 3. 打包方式 A：Gradle（标准做法）

1. 编 Rust 并放进 `jniLibs`：
   ```bash
   cargo ndk -t x86_64 -t arm64-v8a -o packaging/android/app/src/main/jniLibs \
     build -p parallaxe-gui --offline --locked -j 4 --features distribution-release
   ```
2. 在 `packaging/android/` 执行 `gradle assembleDebug`（或 Android Studio 打开）。
   需要 Android SDK Platform 35、Build-Tools 35、JDK 17+；本机当前未安装 Gradle，
   因此这条路径未在本机跑过，脚本仅作骨架保留。

## 4. 离线打包验证

设备验证使用 Android SDK 的 `aapt2` 合并资源、`d8` 合并 Java 字节码，再用 `zipalign` 对齐并用 `apksigner` 签名。当前实际发布链路是 `cargo ndk` + 仓库外 `_apk_build/build_apk_gameactivity.py`；打包前会强制检查 `.so` 含 `parallaxe.distribution-release/v1`，缺标记直接拒绝出包。

## 5. `UnsatisfiedLinkError` 的三类已知原因（启动即退先查这里）

1. **Activity 类型与 `.so` 实现不匹配（最常见）**
   `UnsatisfiedLinkError: ... undefined symbol: ANativeActivity_onCreate` —— Manifest 写了
   `android.app.NativeActivity`，但 crate 用 `game-activity` 编译。两边必须一致。
   打包前可用 `llvm-nm` 校验这三个符号：
   Manifest 声明 GameActivity 时，`.so` 必须导出
   `Java_com_google_androidgamesdk_GameActivity_initializeNativeCode` 与 `android_main`；
   声明 NativeActivity 时必须导出 `ANativeActivity_onCreate`，不一致直接构建失败。
2. **JNI 入口符号被链接器裁掉**（C++/prefab 路径的常见坑，官方要求在链接参数里加
   `-u Java_com_google_androidgamesdk_GameActivity_initializeNativeCode`）。
   Rust 的 `android-activity` 会把该符号放进导出表，本项目已用 `llvm-nm -D` 确认；
   若换构建方式，务必重新确认。
3. **Android 15 的 16 KB 页大小**：`.so` 的 ELF LOAD 段必须按 16 KB 对齐，否则在
   16 KB 页设备上 `dlopen` 失败（同样表现为 `UnsatisfiedLinkError`）。NDK r28 默认
   输出 `Align 0x4000`；可用
   `<ndk>/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-readelf -l <lib.so>` 检查。
   APK 内的 `.so` 若保持压缩，安装时会被解压，不受 `zipalign -P 16` 影响。

## 6. 权限与边界（不变）

Manifest 只声明 `android.permission.INTERNET`，不申请 root、`BIND_VPN_SERVICE`、
存储、账号、定位权限；Android 端报告仍写 `scope = "ANDROID_SIMULATED_USER"`，
只做 App 内观察，不做系统级观测，也不接管系统路由/DNS。
