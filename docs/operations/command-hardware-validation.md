# Shelly Command Hardware Validation

This procedure creates redacted evidence for switch, dimmer, and cover command
delivery. Run it only while physically present. Cover execution requires a clear
view of the moving equipment and a tested physical stop or power-isolation path.

## Setup

Start the daemon with its normal database and credential backend. Bootstrap a
dedicated short-lived actor, then grant only the selected device. Use comfort
risk for switch/dimmer and mechanical risk for cover:

```sh
cargo run --locked -- actor-grant-device-execute ACTOR_ID \
  --device-query 'Kitchen light' --maximum-risk comfort
```

Export the one-time token as `HOMEMAGIC_TOKEN`. First run every scenario without
`--execute`; this exercises complete durable validation and policy without
physical dispatch:

```sh
python3 scripts/hardware-command-smoke.py switch 'Kitchen relay' \
  --output /tmp/switch-validation.json
```

## Physical scenarios

Execution captures the original observation, performs the scenario, and restores
and re-reads original state in `finally`. A scenario cannot pass when cleanup is
unverified.

```sh
python3 scripts/hardware-command-smoke.py switch 'Kitchen relay' --execute \
  --output docs/evidence/hardware/DATE-macos-arm64-shelly-switch-command.json

python3 scripts/hardware-command-smoke.py dimmer 'Kitchen dimmer' --execute \
  --output docs/evidence/hardware/DATE-macos-arm64-shelly-dimmer-command.json
```

The cover scenario sends `stop` before any other movement, briefly exercises
open and close followed by stop, tests calibrated positioning, then sends stop
and restores the original position. The explicit flag records the operator's
precondition; it does not replace physical safety:

```sh
python3 scripts/hardware-command-smoke.py cover 'Office cover' --execute \
  --physical-stop-confirmed \
  --output docs/evidence/hardware/DATE-macos-arm64-shelly-cover-command.json
```

Immediately disable the actor after the session. Run `./scripts/scan-secrets.sh`
before committing reports. A failed cleanup requires manual restoration and an
honest failed report; never edit it into a pass.
