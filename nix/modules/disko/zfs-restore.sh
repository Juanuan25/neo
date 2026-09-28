# Full Neo data restore (the <pool>/neo dataset, i.e. neo.core.volumes.root),
# run in the systemd initrd after the pool is imported and before anything of
# it is mounted. The system itself (root, /nix) is not snapshotted or touched;
# the web UI can switch the boot generation separately.
#
# The web UI schedules a restore by setting a user property on the pool root:
#   neo:restore = <ts>|<live>=<dataset>@<snap>;<live>=<dataset>@<snap>;...
# e.g. 20260928-101500|zroot/neo=zroot/neo@zfs-auto-snap_daily-2026-09-27-0000
#
# `zfs rollback` would destroy every snapshot newer than the target (including
# the safety snapshot), so each live dataset is swapped instead:
#   1. snapshot <live>@neo-prerestore-<ts>    (current state, taken unmounted)
#   2. rename   <live> -> <live>.prev-<ts>    (keeps its newer snapshots)
#   3. clone    <snapshot> -> <live>          (same name: fileSystems still match)
#   4. promote  <live>
# The .prev dataset stays restorable from the UI like any other state.
#
# Result goes to neo:restore-result = ok|<ts>|<detail> or failed|<ts>|<detail>.
# NEO_POOL comes from the unit environment.
# The unit preamble sets -e; failures are handled explicitly below.
set +e -uo pipefail

pool="${NEO_POOL:?NEO_POOL not set}"

req="$(zfs get -H -o value neo:restore "$pool" 2>/dev/null)" || exit 0
if [ -z "$req" ] || [ "$req" = "-" ]; then
  exit 0
fi

# Clear first. A failure below must not repeat on every boot.
zfs inherit neo:restore "$pool"

ts="${req%%|*}"
pairs="${req#*|}"

result() {
  echo "neo-zfs-restore: $1|$2"
  zfs set "neo:restore-result=$1|$ts|$2" "$pool" || true
}

case "$ts" in
  [0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9]) ;;
  *)
    result failed "malformed request: $req"
    exit 0
    ;;
esac

IFS=';' read -r -a items <<<"$pairs"
if [ "${#items[@]}" -eq 0 ]; then
  result failed "no datasets in request"
  exit 0
fi

# Validate everything before touching anything.
for it in "${items[@]}"; do
  live="${it%%=*}"
  src="${it#*=}"
  case "$live" in
    "$pool"/*) ;;
    *)
      result failed "dataset $live is not in pool $pool"
      exit 0
      ;;
  esac
  if ! zfs list -H -o name "$live" >/dev/null 2>&1; then
    result failed "missing dataset $live"
    exit 0
  fi
  if ! zfs list -H -t snapshot -o name "$src" >/dev/null 2>&1; then
    result failed "missing snapshot $src"
    exit 0
  fi
  if zfs list -H -o name "$live.prev-$ts" >/dev/null 2>&1; then
    result failed "$live.prev-$ts already exists"
    exit 0
  fi
done

swapped=()
for it in "${items[@]}"; do
  live="${it%%=*}"
  src="${it#*=}"
  srcds="${src%@*}"
  snap="${src#*@}"
  stash="$live.prev-$ts"
  # A snapshot of the live dataset itself moves with the rename.
  if [ "$srcds" = "$live" ]; then
    srcds="$stash"
  fi

  # Locally set properties (mountpoint=legacy, auto-snapshot, ...) are not
  # inherited by a clone. Creation-only and encryption properties come from
  # the origin and cannot be passed to `zfs clone -o`.
  opts=()
  while IFS=$'\t' read -r prop val; do
    [ -n "$prop" ] || continue
    case "$prop" in
      encryption | keyformat | keylocation | pbkdf2iters | casesensitivity | normalization | utf8only | volsize | volblocksize) continue ;;
    esac
    opts+=(-o "$prop=$val")
  done <<<"$(zfs get -H -p -s local -o property,value all "$live")"

  if ! zfs snapshot "$live@neo-prerestore-$ts"; then
    result failed "snapshot of $live failed; restored: ${swapped[*]:-none}"
    exit 0
  fi
  if ! zfs rename -u "$live" "$stash"; then
    result failed "rename of $live failed; restored: ${swapped[*]:-none}"
    exit 0
  fi
  if ! zfs clone "${opts[@]}" "$srcds@$snap" "$live"; then
    # Put the original back so the machine still boots.
    zfs rename -u "$stash" "$live" || true
    result failed "clone of $srcds@$snap failed; restored: ${swapped[*]:-none}"
    exit 0
  fi
  # Moves the snapshots up to the target onto the restored dataset. Without it
  # the restored dataset would depend on the .prev one. Restoring from a .prev
  # dataset leaves a chain of clones; promote until the whole history (every
  # snapshot older than the target) is back on the live dataset.
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    origin="$(zfs get -H -o value origin "$live")"
    [ -n "$origin" ] && [ "$origin" != "-" ] || break
    if ! zfs promote "$live"; then
      echo "neo-zfs-restore: promote $live failed (restore still in place)"
      break
    fi
  done
  # The old state must not collect or expire auto-snapshots, and must not
  # mount over the restored dataset.
  zfs set com.sun:auto-snapshot=false "$stash" || true
  zfs set canmount=noauto "$stash" || true
  swapped+=("$live")
done

result ok "${swapped[*]} <- ${items[0]#*@}"
