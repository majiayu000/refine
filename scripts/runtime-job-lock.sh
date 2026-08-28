#!/usr/bin/env bash

# Run a complete scheduled Refine workflow under a nofollow, retained-FD lock.
run_refine_runtime_job_locked() {
  local lock_file="${REFINE_RUNTIME_JOB_LOCK_FILE:-${HOME}/.refine/runtime-job.lock}"
  local wait_seconds="${REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS:-14400}"
  local backend="${REFINE_RUNTIME_LOCK_BACKEND:-auto}"
  local lock_parent lock_parent_physical

  [[ "$wait_seconds" =~ ^[0-9]+$ ]] || {
    echo "ERROR: REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS must be a non-negative integer" >&2
    return 1
  }
  [[ "$backend" == "auto" || "$backend" == "flock" || "$backend" == "lockf" ]] || {
    echo "ERROR: REFINE_RUNTIME_LOCK_BACKEND must be auto, flock, or lockf" >&2
    return 1
  }
  [[ "$#" -gt 0 ]] || {
    echo "ERROR: runtime lock requires a child command" >&2
    return 1
  }
  [[ "$lock_file" == /* ]] || {
    echo "ERROR: runtime lock path must be absolute" >&2
    return 1
  }
  lock_parent=$(dirname "$lock_file")
  [[ ! -L "$lock_parent" ]] || {
    echo "ERROR: refusing symlink runtime lock parent: $lock_parent" >&2
    return 1
  }
  mkdir -p "$lock_parent"
  lock_parent_physical=$(cd "$lock_parent" && pwd -P)
  [[ "$lock_parent_physical" == "$lock_parent" ]] || {
    echo "ERROR: runtime lock parent must be canonical: $lock_parent" >&2
    return 1
  }
  chmod 700 "$lock_parent" 2>/dev/null || true
  if [[ "$(uname -s)" == "Darwin" ]]; then
    [[ "$(stat -f '%u' "$lock_parent")" == "$(id -u)" ]] || {
      echo "ERROR: runtime lock parent is not owned by the current user" >&2
      return 1
    }
  else
    [[ "$(stat -c '%u' "$lock_parent")" == "$(id -u)" ]] || {
      echo "ERROR: runtime lock parent is not owned by the current user" >&2
      return 1
    }
  fi

  perl -MFcntl=:DEFAULT,:flock,:mode,F_SETFD -e '
    use strict;
    use warnings;
    my ($path, $wait, @command) = @ARGV;
    sysopen(my $lock, $path, O_RDWR | O_CREAT | O_NOFOLLOW, 0600)
      or die "ERROR: cannot safely open runtime lock $path: $!\n";
    chmod 0600, $lock or die "ERROR: cannot chmod runtime lock $path: $!\n";
    my @opened = stat($lock);
    die "ERROR: runtime lock is not a regular single-link owner file\n"
      unless @opened && S_ISREG($opened[2]) && $opened[3] == 1 && $opened[4] == $<;
    my $deadline = time() + $wait;
    while (!flock($lock, LOCK_EX | LOCK_NB)) {
      exit 75 if time() >= $deadline;
      sleep 1;
    }
    my @named = lstat($path);
    die "ERROR: runtime lock identity changed while acquiring it\n"
      unless @named && $named[0] == $opened[0] && $named[1] == $opened[1]
        && S_ISREG($named[2]) && $named[3] == 1 && $named[4] == $<;
    fcntl($lock, F_SETFD, 0) or die "ERROR: cannot retain runtime lock fd: $!\n";
    $ENV{REFINE_RUNTIME_LOCK_ACTIVE} = 1;
    exec { $command[0] } @command or die "ERROR: cannot exec locked command: $!\n";
  ' "$lock_file" "$wait_seconds" "$@"
}
