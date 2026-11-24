"""A small terminal model, enough to read back what ratatui drew.

Ratatui writes cells with absolute cursor moves, so the raw byte stream is not
the screen. This keeps a grid and applies the handful of sequences a TUI emits:
cursor positioning, erase, and SGR (ignored). Used by the pty tests, and handy
from the command line: python3 scripts/screen.py <binary> [keys...]
"""

import re
import unicodedata


class Screen:
    def __init__(self, cols, rows):
        self.cols = cols
        self.rows = rows
        self.grid = [[" "] * cols for _ in range(rows)]
        self.x = 0
        self.y = 0
        self.saved = (0, 0)

    def _put(self, char):
        width = 2 if unicodedata.east_asian_width(char) in "WF" else 1
        if unicodedata.combining(char):
            return
        if self.x >= self.cols:
            self.x = 0
            self.y += 1
        if 0 <= self.y < self.rows and 0 <= self.x < self.cols:
            self.grid[self.y][self.x] = char
            if width == 2 and self.x + 1 < self.cols:
                self.grid[self.y][self.x + 1] = ""
        self.x += width

    def feed(self, data):
        if isinstance(data, bytes):
            data = data.decode("utf-8", "replace")
        i = 0
        while i < len(data):
            char = data[i]
            if char == "\x1b":
                match = re.match(r"\x1b\[([0-9;?]*)([A-Za-z])", data[i:])
                if match:
                    self._csi(match.group(1), match.group(2))
                    i += len(match.group(0))
                    continue
                match = re.match(r"\x1b[\]P^_].*?(\x07|\x1b\\)", data[i:], re.S)
                if match:
                    i += len(match.group(0))
                    continue
                i += 2
                continue
            if char == "\r":
                self.x = 0
            elif char == "\n":
                self.y = min(self.y + 1, self.rows - 1)
            elif char == "\b":
                self.x = max(0, self.x - 1)
            elif char >= " ":
                self._put(char)
            i += 1

    def _csi(self, params, final):
        args = [int(p) for p in params.split(";") if p.isdigit()]
        if final in "Hf":
            row = (args[0] if args else 1) - 1
            col = (args[1] if len(args) > 1 else 1) - 1
            self.y = min(max(row, 0), self.rows - 1)
            self.x = min(max(col, 0), self.cols - 1)
        elif final == "A":
            self.y = max(0, self.y - (args[0] if args else 1))
        elif final == "B":
            self.y = min(self.rows - 1, self.y + (args[0] if args else 1))
        elif final == "C":
            self.x = min(self.cols - 1, self.x + (args[0] if args else 1))
        elif final == "D":
            self.x = max(0, self.x - (args[0] if args else 1))
        elif final == "G":
            self.x = min(self.cols - 1, (args[0] if args else 1) - 1)
        elif final == "d":
            self.y = min(self.rows - 1, (args[0] if args else 1) - 1)
        elif final == "J":
            mode = args[0] if args else 0
            if mode == 2:
                self.grid = [[" "] * self.cols for _ in range(self.rows)]
            elif mode == 0:
                for xx in range(self.x, self.cols):
                    self.grid[self.y][xx] = " "
                for yy in range(self.y + 1, self.rows):
                    self.grid[yy] = [" "] * self.cols
        elif final == "K":
            mode = args[0] if args else 0
            if mode == 0:
                for xx in range(self.x, self.cols):
                    self.grid[self.y][xx] = " "
            elif mode == 2:
                self.grid[self.y] = [" "] * self.cols

    def text(self):
        return "\n".join("".join(row).rstrip() for row in self.grid)

    def line(self, index):
        return "".join(self.grid[index]).rstrip()
