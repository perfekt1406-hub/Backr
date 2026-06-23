/*
 * Purpose: Declares hash-routed screens mounted inside `App.svelte`.
 * Role: Maps `#/setup`, `#/host`, `#/`, `#/project/…`, `#/settings`, `#/host/settings` for all modes.
 */

import ClientSettingsView from "./components/settings/ClientSettingsView.svelte";
import DashboardView from "./components/dashboard/DashboardView.svelte";
import HostDashboardView from "./components/host/HostDashboardView.svelte";
import HostSettingsView from "./components/host/HostSettingsView.svelte";
import ProjectView from "./components/project/ProjectView.svelte";
import SetupWizard from "./components/setup/SetupWizard.svelte";
import SnapshotBrowserView from "./components/snapshot/SnapshotBrowserView.svelte";

/** `svelte-spa-router` path → component associations sorted depth-first for readability. */
export default {
  "/setup": SetupWizard,
  "/settings": ClientSettingsView,
  "/host/firstrun": HostDashboardView,   /* dev switcher: first-run preview */
  "/host/dashboard": HostDashboardView,  /* dev switcher: with-backups preview */
  "/host/settings": HostSettingsView,
  "/host": HostDashboardView,
  "/": DashboardView,
  "/project/:name": ProjectView,
  "/project/:name/:snapshot": SnapshotBrowserView,
};
