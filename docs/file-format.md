# Titchy File Format

This document defines the current `TCHY` random-access container and the
`TSTR` packet archive used by the CLI.

## Conventions

- Multi-byte metadata integers are unsigned little-endian.
- Bases, base IDs, deviations, and pair payloads are packed MSB-first.
- Original sample bit index zero is the most significant bit of sample zero.
- A chunk contains `samples_per_chunk * bits_per_sample` bits.
- Unused low bits in the last byte of a non-byte-aligned base are zero.

## Versioning

Byte 4 of a `TCHY` file is the current format marker, `2`. Readers reject every
other value. The project is pre-1.0 and does not maintain compatibility with
experimental container layouts.

The format marker covers the container structure, not the Titchy algorithm.
Changes to offsets, index records, or metadata require a new marker.

## Container Layout

```text
+-----------------------+
| Fixed header          | 38 bytes
+-----------------------+
| Split index           | split_count * 24 bytes
+-----------------------+
| Persisted dictionary  | dictionary_count * base_byte_len
+-----------------------+
| Split record 0        |
+-----------------------+
| Split record 1        |
+-----------------------+
| ...                   |
+-----------------------+
```

### Fixed Header

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 4 | ASCII magic `TCHY` |
| 4 | 1 | Format marker `2` |
| 5 | 1 | Bits per sample |
| 6 | 1 | Samples per chunk |
| 7 | 1 | Sample endian tag: little `0`, big `1` |
| 8 | 2 | Chunks per split |
| 10 | 4 | New-base threshold in parts per million |
| 14 | 8 | Active dictionary byte limit; `u64::MAX` means unlimited |
| 22 | 8 | Original unpadded sample count |
| 30 | 4 | Persisted dictionary base count |
| 34 | 4 | Split count |

The endian tag describes conversion between byte-aligned raw samples and their
numeric bit patterns. It does not change bit packing inside the container.

### Split Index

Each split has one 24-byte index entry:

| Relative offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | First global chunk index |
| 8 | 4 | Number of chunks in the split |
| 12 | 8 | Absolute byte offset of the split record |
| 20 | 4 | Split record length in bytes |

Entries are stored in split order. They let `IndexedTitchyReader` locate a
chunk without scanning preceding split payloads.

### Persisted Dictionary

Each dictionary base occupies:

```text
base_byte_len = ceil(bits_per_sample * samples_per_chunk / 8)
```

Entries are stored in monotonically assigned base-ID order. A base evicted
from the encoder's active LRU remains in this persisted dictionary. If it is
encountered after eviction, it receives a new ID and another persisted entry.

The active memory limit affects encoder lookup state only. It does not remove
data required to decode previously emitted pairs.

### Split Record

Every split record starts with:

| Relative offset | Size | Field |
| ---: | ---: | --- |
| 0 | 1 | Base-ID width `l_id` |
| 1 | 1 | Deviation width `l_d` |
| 2 | 2 | Pair count |
| 4 | 4 | Packed payload length in bytes |
| 8 | variable | Packed pairs |

Each pair occupies exactly `l_id + l_d` bits:

```text
+------------------+------------------+
| base ID          | deviation        |
| l_id bits        | l_d bits         |
+------------------+------------------+
```

Pairs are adjacent with no byte alignment between them. Padding is permitted
only after the final pair to complete its last storage byte.

## Base and Deviation Ordering

For each chunk, `l_d` low-order bits are distributed as evenly as possible
across samples. Remainder bits are assigned to the last samples. Selected bits
are emitted one bit level at a time, from least significant upward, and within
each level from the first sample to the last eligible sample.

The base is the original chunk with every selected position cleared. Decoding
resolves the base ID and writes deviation bits back through the same position
sequence.

The final partial chunk is padded with zero-valued samples. The header's
original sample count removes those values after decoding.

## Random Access

For a requested sample, the reader:

1. Computes the global chunk index.
2. Locates the split through the split index.
3. Reads the split header to obtain `l_id` and `l_d`.
4. Reads only the requested pair's packed bytes.
5. Resolves its persisted base.
6. Reconstructs the containing chunk.

For a requested bit, the reader first determines whether the original position
belongs to the deviation. It then reads either the one deviation bit or the
base ID plus one dictionary bit.

Range access reconstructs only chunks intersecting the requested sample range.

## Streaming Packet Payload

The paper-compatible packet begins with:

```text
bit 7     parameters-present flag
bits 0-6  number of new bases, 0..127
```

When parameters are present, one-byte `l_id` and `l_d` fields follow. The rest
is one MSB-first bitstream containing new bases followed by `(base ID,
deviation)` pairs.

The packet transport must provide its chunk count because the paper payload
does not encode it.

## TSTR Archive

`TSTR` is CLI transport framing, not part of the compression algorithm. Its
header stores:

| Field | Size |
| --- | ---: |
| ASCII magic `TSTR` | 4 bytes |
| Archive format marker `1` | 1 byte |
| Bits per sample | 1 byte |
| Samples per chunk | 1 byte |
| Chunks per split | 2 bytes |
| Original sample count | 8 bytes |
| Packet count | 4 bytes |

Each packet is prefixed by a two-byte chunk count and four-byte payload length.
Real deployments can replace this archive with equivalent network or link
framing.
