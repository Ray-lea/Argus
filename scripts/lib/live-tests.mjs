// The ignored tests that need a real machine and may change it: live
// collectors, scheduled tasks, the Recycle Bin, owned helper processes,
// windows on an inactive desktop. They are marked #[ignore] so that
// `cargo test` on a development machine leaves them out; the nightly workflow
// runs them on a disposable runner (scripts/live-tests.mjs). Tests that are
// only child processes of another test are not listed: run on their own they
// fail by design.
//
// Values in braces are filled in by the runner: {argus} the development
// build of argus.exe, {account} the current account, {secondVolume} a
// writable folder on another volume than %TEMP%.
export const GROUPS = [
  {
    name: "collectors",
    about: "processes, GPUs, disks, sensors, CPU counters and leak findings read from this machine",
    cargo: ["-p", "argus-core", "--lib"],
    tests: [
      "collector::tests::a_gpu_missing_from_a_stale_list_returns_after_one_sample",
      "collector::tests::live_gpu_snapshot_keeps_process_and_device_engine_totals_consistent",
      "hardware::disk::tests::live_controllers",
      "hardware::disk::tests::live_inventory",
      "hardware::disk::tests::live_performance_paths",
      "hardware::disk::tests::live_power_state",
      "hardware::disk::tests::live_removal_cancellation",
      "hardware::disk::tests::live_volume_fallback",
      "hardware::gpu::metadata::tests::live_adapter_metadata",
      "hardware::gpu::power::tests::live_adapter_power_state",
      "hardware::gpu::tests::live_driver_access",
      "hardware::sensors::tests::live_cpu_clocks",
      "leaks::tests::live_snapshot_reports_extreme_handle_counts",
      "leaks::tests::observe_cost",
      "system::tests::live_frozen_idle_uses_legacy_counters",
    ],
  },
  {
    name: "owned processes",
    about: "watch identity of owned helper processes, a startup file sent to the Recycle Bin",
    cargo: ["-p", "argus-core", "--lib"],
    tests: [
      "watch::identity::tests::two_copies_stay_distinct_and_restarting_the_same_copy_keeps_program_identity",
      "watch::program::tests::real_idle_instances_restart_and_same_name_other_folder_remain_separate",
      "startup::tests::a_startup_file_goes_to_the_recycle_bin",
    ],
  },
  // The soaks count the process's own GDI objects or handles: each runs in a
  // process of its own, as their #[ignore] notes ask, so that what an earlier
  // test left behind does not count as a leak.
  {
    name: "icon soak",
    about: "GDI objects of the process while icons are extracted again and again",
    cargo: ["-p", "argus-core", "--lib"],
    tests: ["meta::category_tests::resource_soak_icon_extraction"],
  },
  {
    name: "history across volumes",
    about: "an interrupted move of the history folder to another volume",
    cargo: ["-p", "argus-core", "--lib"],
    tests: ["history::tests::retention::interrupted_move_across_volumes"],
    env: { ARGUS_HISTORY_TEST_SECOND_VOLUME: "{secondVolume}" },
  },
  {
    name: "scheduled tasks",
    about: "administrator-startup tasks registered, checked and removed",
    cargo: ["-p", "argus-service", "--lib"],
    tests: ["admin::task::tests::vm_registration_security"],
    env: { ARGUS_VM_TEST: "1", ARGUS_VM_ACCOUNT: "{account}" },
  },
  {
    name: "scheduled tasks as a standard user",
    about: "a standard account revoking its own administrator-startup tasks",
    cargo: ["-p", "argus-service", "--lib"],
    tests: ["admin::task::tests::ordinary_user_revokes_tasks_without_runtime_directories"],
    requires: ["standard rights"],
  },
  {
    name: "privileged helpers",
    about: "the protected helper report, sensor thread affinity",
    cargo: ["-p", "argus-service", "--lib"],
    tests: ["diagnostics::tests::protected_helper_report_round_trip", "sensors::cpu::tests::affinity_cadence_without_driver_access"],
  },
  {
    name: "broker soak",
    about: "handles of the process while clients connect to and leave the local broker",
    cargo: ["-p", "argus-service", "--lib"],
    tests: ["pipe::tests::resource_soak_requests_disconnects_and_shutdown"],
  },
  {
    name: "desktop shell",
    about: "a program started as a child of the desktop shell",
    cargo: ["-p", "argus-app", "--lib"],
    tests: ["tasks::tests::live_shell_child"],
  },
  {
    // Without a split token the shell itself is elevated, and the helper
    // refuses to run a program "at standard rights" with it (0x20000101).
    name: "standard rights from an elevated process",
    about: "the helper that starts programs at standard rights under the desktop shell",
    cargo: ["-p", "argus-app", "--lib"],
    tests: ["tasks::tests::live_shell_helper"],
    env: { ARGUS_TASKS_HOST: "{argus}" },
    requires: ["split token"],
  },
  {
    name: "process watch",
    about: "a watched process observed and deleted through the real sampler",
    cargo: ["-p", "argus-app", "--test", "process_watch"],
    tests: ["observes_and_deletes_through_the_real_sampler"],
  },
  {
    name: "window activation",
    about: "activation, keep-on-top and window motion with real windows on an inactive desktop",
    cargo: ["-p", "argus", "--lib"],
    tests: [
      "activation::pressure_tests::an_unresponsive_topmost_window_does_not_cover_an_explicitly_activated_window",
      "activation::pressure_tests::deactivation_keeps_the_newly_active_normal_window_above_argus",
      "activation::pressure_tests::keeping_on_top_outlasts_deactivation_and_ends_when_turned_off",
      "activation::pressure_tests::normal_and_previously_pinned_windows_keep_their_placement_policy",
      "activation::pressure_tests::only_an_ordinary_window_above_covers_argus",
      "activation::pressure_tests::switching_to_a_topmost_window_and_hiding_both_end_temporary_elevation",
      "window_motion::native_tests::disabling_window_motion_releases_native_commands_and_cleans_an_active_fade",
      "window_motion::native_tests::window_placement_survives_hiding_and_reopening",
    ],
  },
];

/** The group's environment with the braces filled in; throws on an unknown name. */
export function environmentOf(group, values) {
  return Object.fromEntries(
    Object.entries(group.env ?? {}).map(([key, value]) => [
      key,
      value.replace(/\{(\w+)\}/g, (_, name) => {
        if (values[name] === undefined) throw new Error(`${group.name}: no value for {${name}}`);
        return values[name];
      }),
    ]),
  );
}

/** Which placeholders the chosen groups need. */
export function placeholders(groups) {
  const names = new Set();
  for (const group of groups) for (const value of Object.values(group.env ?? {})) for (const [, name] of value.matchAll(/\{(\w+)\}/g)) names.add(name);
  return names;
}

/** `cargo test` arguments for one group: only its ignored tests, exactly, one at a time. */
export function cargoArgs(group) {
  return ["test", "--locked", ...group.cargo, "--", "--ignored", "--exact", "--test-threads=1", ...group.tests];
}

/**
 * Whether an administrator signed in here gets a split token, so that the
 * desktop shell runs at standard rights: UAC on, and for the built-in
 * Administrator (RID 500) also Admin Approval Mode, which Windows leaves off
 * for that account. Values as `reg query` prints them, null when absent.
 */
export function splitToken({ enableLua, filterAdministratorToken, sid }) {
  if (enableLua === "0x0") return false;
  return !/-500$/.test(sid ?? "") || filterAdministratorToken === "0x1";
}

/**
 * Why a group cannot run on this machine, or null. `facts` says what the
 * machine offers: `elevated`, `splitToken`, and the placeholder values.
 */
export function unmet(group, facts) {
  for (const need of group.requires ?? []) {
    if (need === "standard rights" && facts.elevated) return "needs a process at standard rights; this one is elevated";
    if (need === "split token" && !facts.splitToken)
      return "needs an account with a split token: here the desktop shell runs with the full administrator token (UAC off, or the built-in Administrator)";
  }
  for (const name of placeholders([group])) if (facts.values[name] === undefined) return facts.missing?.[name] ?? `no value for {${name}}`;
  return null;
}

/**
 * The outcome of each listed test in libtest's output. A test's own output
 * (from child processes it starts) can come between `test NAME ... ` and its
 * `ok`, so a started test counts as passed unless it is in the failures list
 * or the run ended without its `test result:` line (a crash: `crashed`). A
 * listed test that never started — renamed, or the process died before it —
 * is `missing`. Both fail like a failure does.
 */
export function outcomes(group, output) {
  const started = new Map();
  for (const match of output.matchAll(/^test (\S+) \.\.\. ?(ok|FAILED|ignored)?/gm)) started.set(match[1], match[2] ?? null);
  // The second "failures:" block lists the names, indented four spaces.
  const failed = new Set();
  for (const block of output.matchAll(/^failures:\r?\n((?: {4}\S+\r?\n?)+)/gm))
    for (const name of block[1].split(/\r?\n/)) if (name.trim()) failed.add(name.trim());
  const finished = /^test result: /m.test(output);
  return group.tests.map((test) => {
    if (!started.has(test)) return { test, result: "missing" };
    const same = started.get(test);
    if (same === "FAILED" || failed.has(test)) return { test, result: "failed" };
    if (same === "ok" || finished) return { test, result: "passed" };
    return { test, result: "crashed" };
  });
}
