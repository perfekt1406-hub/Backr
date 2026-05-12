/*
 * Purpose: DTO for [`get_system_info`] — local hostname, distro, kernel, arch, user, sample time.
 * Role: Consumed by the dashboard system info panel beside rsync output.
 */

export type SystemInfo = {
  hostname: string | null;
  os_pretty: string;
  kernel_release: string | null;
  arch: string;
  user: string | null;
  sampled_at_rfc3339: string;
};
