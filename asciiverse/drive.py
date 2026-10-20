#!/usr/bin/env python3
"""Records asciiverse/armcom2.cast, the catalog preview, at 100x30.

Usage: drive.py <armcom2 binary> <out.cast> [seconds] [campaign]

asciinema rec runs `armcom2 --battle <campaign>` (by name, from a throwaway
folder on PATH, with the game's own options set to no sound and a short
Message Pause) in a detached 100x30 tmux session. This script reads the
pane and plays the way a player would: in the first Command Phase the
gunner goes on Operate Gun and the loader on Reload; in each Shooting
Phase the gunner fires at the first identified target he can, turning the
turret onto it and changing from AP or smoke to a shell that works, and
again while the rate of fire is kept; every attack window is read for a
moment. After `seconds` (default 50) it saves and quits from the game
menu, or stops sooner when the battle ends on its own. What each window
said goes to <out.cast>.log, to choose the best of several takes (the
catalog's was the best of twenty). Needs tmux and asciinema 3."""
import os, shutil, subprocess, sys, tempfile, time

binary, out = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
secs = float(sys.argv[3]) if len(sys.argv) > 3 else 50.0
campaign = sys.argv[4] if len(sys.argv) > 4 else "pattons_best"

S = f"armcom2-preview-{os.getpid()}"
TMUX = ["tmux", "-L", S]
home = tempfile.mkdtemp()
shutil.copy(binary, os.path.join(home, "armcom2"))
# the game's own options: sound off, Message Pause short
data = os.path.join(home, ".local", "share", "armoured-commander-ii")
os.makedirs(data)
with open(os.path.join(data, "settings.json"), "w") as f:
    f.write('{"sounds_enabled": false, "master_volume": 10, "animation_speed": 1, "message_pause": 0, "keyboard": 0}')
env = dict(os.environ, HOME=home, PATH=home + ":" + os.environ["PATH"],
           TERM="xterm-256color", COLORTERM="truecolor", LANG="C.UTF-8",
           USER="player", LOGNAME="player",
           # no sound while recording
           PIPEWIRE_REMOTE="/nonexistent", PULSE_SERVER="unix:/nonexistent")
for k in ("VTE_VERSION", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "TMUX", "XDG_DATA_HOME", "RUST_LOG"):
    env.pop(k, None)

def key(*k):
    subprocess.run(TMUX + ["send-keys", "-t", S, *k])

def screen():
    r = subprocess.run(TMUX + ["capture-pane", "-p", "-t", S], capture_output=True, text=True)
    return [l.ljust(100) for l in (r.stdout.split("\n") + [""] * 30)[:30]]

def running():
    return subprocess.run(TMUX + ["has-session", "-t", S], capture_output=True).returncode == 0

def right(rows):
    # the right column's text, where the phase and the context panel are
    return "\n".join(r[-17:-1].strip() for r in rows)

def order(rows, title):
    # the line under a crewman's name: position title, name, order
    for i, r in enumerate(rows[:-2]):
        if r[1:].startswith(title):
            return rows[i + 2][1:22].strip()
    return None

def set_order(rows, title, want):
    """Step down to the position, then round its orders to `want`."""
    titles = [r[1:22].split("  ")[0] for r in rows[3:21:3]]
    n = next((i for i, t in enumerate(titles) if t.startswith(title)), None)
    if n is None:
        return
    for _ in range(n):
        key("s"); time.sleep(0.25)
    for _ in range(10):
        if order(screen(), title) == want:
            break
        key("d"); time.sleep(0.35)
    for _ in range(n):
        key("w"); time.sleep(0.15)

cmd = ("env TERM=xterm-256color asciinema rec --overwrite -q --window-size 100x30 --idle-time-limit 2"
       " --title 'Armoured Commander II' --capture-env TERM,COLORTERM,LANG"
       f" -c 'armcom2 --battle {campaign}' " + out)
subprocess.run(TMUX + ["new-session", "-d", "-s", S, "-x", "100", "-y", "30",
                       "-c", home, cmd], env=env, check=True)
# what each window said, with its time, for choosing the best take
log = open(out + ".log", "w")
def note(what):
    log.write(f"{time.time() - t0:5.1f} {' '.join(what.split())}\n"); log.flush()

try:
    t0 = time.time()
    time.sleep(2.5)
    orders_set, again = False, 0
    while running() and time.time() - t0 < secs:
        rows = screen()
        text, side = "".join(rows), right(rows)
        if "Fire Again" in text and again < 2:
            time.sleep(1.2); key("f"); again += 1; time.sleep(0.5)
        elif "Continue" in text or "Skip" in text:
            # an attack or firepower window: read it, then go on
            time.sleep(1.4)
            seen = " | ".join(r[23:83].strip() for r in screen() if r[23:83].strip())
            note(seen)
            key("Enter"); again = 0; time.sleep(0.4)
        elif "Command Phase" in side:
            if not orders_set:
                time.sleep(0.8)
                set_order(rows, "Gunner", "Operate Gun")
                set_order(screen(), "Loader", "Reload")
                orders_set = True
            time.sleep(0.8); key("Space"); time.sleep(0.6)
        elif "Shooting Phase" in side:
            # the first target the gun can fire on, the panel saying why
            # not: turn the turret onto it, or change to a shell that works
            fired = False
            for _ in range(6):
                key("d"); time.sleep(0.35)
                s = right(screen())
                for _ in range(6):
                    if "covered" not in s:
                        break
                    key("e"); time.sleep(0.4)
                    s = right(screen())
                for _ in range(3):
                    if "no effect" not in s and "No more ammo" not in s:
                        break
                    key("c"); time.sleep(0.4)
                    s = right(screen())
                # only at what the crew has identified, so the preview
                # shows what it fires at
                if "Ready to fire" not in s or "Unspotted" in s:
                    continue
                time.sleep(0.6); key("f"); time.sleep(0.6)
                # the window names the shell before it is fired: not smoke
                for _ in range(3):
                    t = "".join(r[23:83] for r in screen())
                    if "w/" not in t or "Smoke" not in t.split("w/")[1][:30] or "Cancel" not in t:
                        break
                    time.sleep(0.6); key("BSpace"); time.sleep(0.4); key("c"); time.sleep(0.4); key("f"); time.sleep(0.6)
                if "Continue" in "".join(screen()) or "Skip" in "".join(screen()):
                    fired = True
                    break
            if not fired:
                time.sleep(0.5); key("Space"); time.sleep(0.5)
        elif "Movement Phase" in side or "Crew Action" in side:
            time.sleep(0.5); key("Space"); time.sleep(0.5)
        else:
            time.sleep(0.4)
    if running():
        # close any open window, then Save and Quit from the game menu
        time.sleep(1.5)
        for _ in range(4):
            t = "".join(screen())
            if "Continue" not in t and "Skip" not in t:
                break
            key("Enter"); time.sleep(0.6)
        key("Escape"); time.sleep(0.8); key("q")
    for _ in range(50):
        if not running():
            break
        time.sleep(0.1)
finally:
    subprocess.run(TMUX + ["kill-server"], capture_output=True)
    # asciinema detaches from tmux and ignores SIGTERM while its command
    # runs, so end the game under this recording; asciinema then finishes
    def descendants(pid):
        r = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True)
        kids = [int(p) for p in r.stdout.split()]
        return kids + [d for k in kids for d in descendants(k)]
    r = subprocess.run(["pgrep", "-f", "^asciinema rec .*" + out], capture_output=True, text=True)
    for rec in (int(p) for p in r.stdout.split()):
        for pid in descendants(rec):
            subprocess.run(["kill", str(pid)], capture_output=True)
    log.close()
    shutil.rmtree(home, ignore_errors=True)
