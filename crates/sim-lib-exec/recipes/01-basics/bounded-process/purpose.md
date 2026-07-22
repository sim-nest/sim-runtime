This recipe records a host process run as a bounded operation: a caller supplies
argv, a working directory root, a timeout, an output limit, and optional stdin.
The host captures stdout and stderr separately and reports the exit code without
treating the child process as SIM evaluation.
