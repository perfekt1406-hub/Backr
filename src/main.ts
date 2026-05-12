/*
 * Purpose: Svelte bootstrap that mounts the root shell inside `#app`.
 * Role: Registers Tailwind-driven styles once via `./app.css` side-effect import.
 */

import "./app.css";

import { mount } from "svelte";

import App from "./App.svelte";

/**
 * Mounts the root layout into `#app`.
 *
 * External: `mount` from `svelte` wires reactive reconciliation onto `#app`.
 */
mount(App, { target: document.getElementById("app")! });
