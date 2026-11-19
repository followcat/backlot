#!/usr/bin/env python3
"""Send a click or drag to one explicit X display.

The rig never assumes :0. Pass --display (or RIG_DISPLAY) or nothing
happens. Requires python-xlib for real input; --dry-run works without it.

    xclick.py --display :90 click 400 300
    xclick.py --display :90 click 400 300 --button 3
    xclick.py --display :90 drag 100 100 400 300
    xclick.py --display :90 --dry-run drag 1 2 3 4
"""

import argparse
import os
import sys
import time


def display_name(args):
    name = args.display or os.environ.get("RIG_DISPLAY")
    if not name:
        sys.exit(
            "xclick: no display; pass --display :N or set RIG_DISPLAY "
            "(it never assumes :0)"
        )
    return name


def load_xlib():
    try:
        from Xlib import X, display
        from Xlib.ext import xtest
    except ImportError:
        sys.exit("xclick: python-xlib is required for real input (pip install python-xlib)")
    return X, display, xtest


def real_input(args):
    name = display_name(args)
    X, display, xtest = load_xlib()
    try:
        d = display.Display(name)
    except Exception as exc:  # Xlib raises its own connection error type
        sys.exit(f"xclick: cannot open display {name}: {exc}")

    def move(x, y):
        xtest.fake_input(d, X.MotionNotify, x=x, y=y)
        d.sync()

    def button(number, press):
        kind = X.ButtonPress if press else X.ButtonRelease
        xtest.fake_input(d, kind, number)
        d.sync()

    if args.command == "click":
        move(args.x, args.y)
        button(args.button, True)
        button(args.button, False)
    else:
        move(args.x1, args.y1)
        button(args.button, True)
        steps = max(args.steps, 1)
        for i in range(1, steps + 1):
            x = args.x1 + (args.x2 - args.x1) * i // steps
            y = args.y1 + (args.y2 - args.y1) * i // steps
            move(x, y)
            time.sleep(0.01)
        button(args.button, False)
    d.close()


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Click or drag on one explicit X display."
    )
    parser.add_argument("--display", help="X display, e.g. :90 (or RIG_DISPLAY)")
    parser.add_argument("--dry-run", action="store_true", help="parse and print only")
    sub = parser.add_subparsers(dest="command", required=True)

    click = sub.add_parser("click", help="click at X Y")
    click.add_argument("x", type=int)
    click.add_argument("y", type=int)
    click.add_argument("--button", type=int, choices=(1, 2, 3), default=1)

    drag = sub.add_parser("drag", help="drag from X1 Y1 to X2 Y2")
    drag.add_argument("x1", type=int)
    drag.add_argument("y1", type=int)
    drag.add_argument("x2", type=int)
    drag.add_argument("y2", type=int)
    drag.add_argument("--button", type=int, choices=(1, 2, 3), default=1)
    drag.add_argument("--steps", type=int, default=10)

    args = parser.parse_args(argv)
    name = display_name(args)

    if args.dry_run:
        if args.command == "click":
            print(f"click {name} {args.x} {args.y} button={args.button}")
        else:
            print(
                f"drag {name} {args.x1} {args.y1} {args.x2} {args.y2} "
                f"button={args.button} steps={args.steps}"
            )
        return 0

    real_input(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
