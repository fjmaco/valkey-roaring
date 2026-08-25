---
layout: home

hero:
  name: "Valkey Roaring"
  text: "Roaring Bitmaps for Valkey"
  tagline: Compressed bitmaps as native server types — 51 commands, 32/64-bit variants, and cross-language binary export. Runs on Valkey 8.1+ and Redis 7.4+.
  actions:
    - theme: brand
      text: Getting Started
      link: /guide/getting-started
    - theme: alt
      text: What Is a Roaring Bitmap
      link: /guide/what-is-roaring-bitmap
    - theme: alt
      text: Commands
      link: /commands/

features:
  - icon: 📦
    title: Cross-language binary export
    details: R.EXPORT and R.IMPORT speak the CRoaring portable format — move bitmaps between Valkey and Java, Go, Python, C++, or Rust services without intermediate integer arrays.
    link: /guide/export-import
  - icon: 🧮
    title: Two value ranges
    details: 32-bit (R.*) and 64-bit (R64.*) command families with identical semantics, generated from a single generic implementation that cannot drift apart.
    link: /commands/
  - icon: ⚡
    title: Fast set operations
    details: AND, OR, XOR, NOT, ANDOR, DIFF, DIFF1 and ONE run on compressed containers — several times faster than native bitmaps on real datasets.
    link: /guide/performance
  - icon: 🔁
    title: Drop-in redis-roaring compatibility
    details: The command surface follows redis-roaring, kept reply-identical by differential testing against the original module — and the same .so loads on both Valkey and Redis, so existing deployments migrate unchanged.
  - icon: 💾
    title: Persistence and replication
    details: RDB snapshots, AOF with the default preamble configuration, verbatim replication to replicas, DUMP/RESTORE and COPY support.
    link: /guide/persistence-and-replication
  - icon: 🧪
    title: Battle-tested
    details: Unit and property tests, fuzzing, a 283-assertion integration suite, and external validation against real datasets and reference implementations on every push.
---
