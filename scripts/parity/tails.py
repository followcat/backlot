"""Exact detach tails with fixture-authorized synthesized drain segments."""
from collections import Counter
import re

RELEASE = re.compile(rb'\x1b\[[0-9:;]*:3u')


def matches(value, segments):
    data = bytes.fromhex(value)
    offset = 0
    for segment in segments:
        if set(segment) == {'bytes_hex'}:
            expected = bytes.fromhex(segment['bytes_hex'])
            if data[offset:offset + len(expected)] != expected:
                return False
            offset += len(expected)
        elif set(segment) == {'synthesized_releases_hex'}:
            expected = [bytes.fromhex(item) for item in segment['synthesized_releases_hex']]
            if not expected or not all(RELEASE.fullmatch(item) for item in expected):
                raise ValueError('fixture drain must contain complete CSI-u releases')
            length = sum(map(len, expected))
            chunk = data[offset:offset + length]
            actual = RELEASE.findall(chunk)
            if b''.join(actual) != chunk or Counter(actual) != Counter(expected):
                return False
            offset += length
        else:
            raise ValueError('unknown detach-tail fixture segment')
    return offset == len(data)


def teardown_equivalent(source, candidate, expected=None):
    # No exception without a source-derived, explicit drain whitelist.
    if expected is None:
        return source == candidate
    return (len(source) == len(candidate) == len(expected)
            and all(matches(a, spec) and matches(b, spec)
                    for a, b, spec in zip(source, candidate, expected)))
