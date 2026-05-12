/*
 * Purpose: Declares hash-routed screens mounted inside `App.svelte`.
 * Role: Maps `#/setup`, `#/host`, `#/`, `#/project/…` for laptop vs backup-host dashboards.
 */

import DashboardView from "./components/dashboard/DashboardView.svelte";
import HostDashboardView from "./components/host/HostDashboardView.svelte";
import ProjectView from "./components/project/ProjectView.svelte";
import SetupWizard from "./components/setup/SetupWizard.svelte";
import SnapshotBrowserView from "./components/snapshot/SnapshotBrowserView.svelte";

/** `svelte-spa-router` path → component associations sorted depth-first for readability. */
export default {
  "/setup": SetupWizard,
  "/host": HostDashboardView,
  "/": DashboardView,
  "/project/:name": ProjectView,
  "/project/:name/:snapshot": SnapshotBrowserView,
};
