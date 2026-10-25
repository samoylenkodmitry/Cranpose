"""Turns a Rust v0 mangled symbol name into a linker section-name pattern that
matches the same function in any build: crate hashes and back-references
become `*`, and so does the crate that instantiated a generic function.

The parser follows the grammar of the v0 scheme
(https://doc.rust-lang.org/rustc/symbol-mangling/v0.html), so letters and
digits inside identifiers stay as they are.
"""

import string

BASE62 = string.digits + string.ascii_letters
BASIC_TYPES = set("abcdefhijlmnostuvxyzp")


class Parser:
    def __init__(self, name):
        if not name.startswith("_R"):
            raise ValueError(f"not a v0 name: {name}")
        self.text = name
        self.pos = 2
        self.out = ["_R"]

    def peek(self):
        return self.text[self.pos] if self.pos < len(self.text) else ""

    def take(self):
        char = self.text[self.pos]
        self.pos += 1
        return char

    def emit(self, piece):
        self.out.append(piece)

    def base62(self):
        """Skips a base-62 number with its closing underscore and returns it."""
        start = self.pos
        while self.peek() in BASE62 and self.peek() != "":
            self.pos += 1
        if self.take() != "_":
            raise ValueError(f"bad base-62 number at {start} in {self.text}")
        return self.text[start:self.pos]

    def decimal(self):
        """A decimal number: a lone 0, or digits that do not start with 0."""
        start = self.pos
        if self.peek() == "0":
            self.pos += 1
            return 0
        while self.peek().isdigit():
            self.pos += 1
        return int(self.text[start:self.pos])

    def undisambiguated_identifier(self):
        punycode = self.peek() == "u"
        if punycode:
            self.emit(self.take())
        length = self.decimal()
        self.emit(str(length))
        if self.peek() == "_":
            self.emit(self.take())
        self.emit(self.text[self.pos:self.pos + length])
        self.pos += length

    def identifier(self, crate_root):
        if self.peek() == "s":
            self.take()
            number = self.base62()
            # A crate's disambiguator is a hash of the build; any other one
            # numbers the items of one name in a scope and stays.
            self.emit("s*_" if crate_root else "s" + number)
        self.undisambiguated_identifier()

    def backref(self):
        self.take()
        self.base62()
        self.emit("*")

    def path(self):
        tag = self.peek()
        if tag == "C":
            self.emit(self.take())
            self.identifier(crate_root=True)
        elif tag == "M":
            self.emit(self.take())
            self.impl_path()
            self.type()
        elif tag == "X":
            self.emit(self.take())
            self.impl_path()
            self.type()
            self.path()
        elif tag == "Y":
            self.emit(self.take())
            self.type()
            self.path()
        elif tag == "N":
            self.emit(self.take())
            self.emit(self.take())
            self.path()
            self.identifier(crate_root=False)
        elif tag == "I":
            self.emit(self.take())
            self.path()
            while self.peek() != "E":
                self.generic_arg()
            self.emit(self.take())
        elif tag == "B":
            self.backref()
        else:
            raise ValueError(f"bad path at {self.pos} in {self.text}")

    def impl_path(self):
        # An impl's disambiguator numbers the impl blocks of its module, so
        # an impl block added or merged above renumbers it; the pattern
        # leaves it out, and an impl without one matches as well.
        if self.peek() == "s":
            self.take()
            self.base62()
        self.emit("*")
        self.path()

    def generic_arg(self):
        tag = self.peek()
        if tag == "L":
            self.emit(self.take() + self.base62())
        elif tag == "K":
            self.emit(self.take())
            self.const()
        else:
            self.type()

    def const(self):
        tag = self.peek()
        if tag == "p":
            self.emit(self.take())
        elif tag == "B":
            self.backref()
        else:
            self.type()
            start = self.pos
            if self.peek() == "n":
                self.pos += 1
            while self.peek() in string.hexdigits and self.peek() != "":
                self.pos += 1
            if self.take() != "_":
                raise ValueError(f"bad const at {start} in {self.text}")
            self.emit(self.text[start:self.pos])

    def binder(self):
        if self.peek() == "G":
            self.emit(self.take() + self.base62())

    def type(self):
        tag = self.peek()
        if tag in BASIC_TYPES:
            self.emit(self.take())
        elif tag in "CMXYNI":
            self.path()
        elif tag == "B":
            self.backref()
        elif tag == "A":
            self.emit(self.take())
            self.type()
            self.const()
        elif tag in "SPO":
            self.emit(self.take())
            self.type()
        elif tag in "RQ":
            self.emit(self.take())
            if self.peek() == "L":
                self.emit(self.take() + self.base62())
            self.type()
        elif tag == "T":
            self.emit(self.take())
            while self.peek() != "E":
                self.type()
            self.emit(self.take())
        elif tag == "F":
            self.emit(self.take())
            self.binder()
            if self.peek() == "U":
                self.emit(self.take())
            if self.peek() == "K":
                self.emit(self.take())
                if self.peek() == "C":
                    self.emit(self.take())
                else:
                    self.undisambiguated_identifier()
            while self.peek() != "E":
                self.type()
            self.emit(self.take())
            self.type()
        elif tag == "D":
            self.emit(self.take())
            self.binder()
            while self.peek() != "E":
                self.path()
                while self.peek() == "p":
                    self.emit(self.take())
                    self.undisambiguated_identifier()
                    self.type()
            self.emit(self.take())
            self.emit(self.take() + self.base62())
        else:
            raise ValueError(f"bad type at {self.pos} in {self.text}")


def pattern(name):
    """The pattern of the v0 name `name`."""
    parser = Parser(name)
    if parser.peek().isdigit():
        parser.emit(str(parser.decimal()))
    parser.path()
    # What follows is the instantiating crate, or a vendor suffix.
    return "".join(parser.out) + "*"
