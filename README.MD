## Autonomic
[![CI](https://github.com/geo-mak/autonomic/actions/workflows/ci.yml/badge.svg)](https://github.com/geo-mak/autonomic/actions/workflows/ci.yml)

A general purpose infrastructure control plane.

Created out of curiosity and for personal use as part of ongoing transition to Rust for implementing infrastructure <br>
software.

The name is inspired by autonomic computing, a set of ideas I was tinkering with when I wrote this thing.<br>
Implementations vary, but the basic idea is that a particular system can monitor and adjust itself according to <br>
specifications and policies with minimal manual control. Architectures also vary, ranging from centralized control <br>
layers to decentralized embedded control elements.

### Core Design Principles:
* Modular and composable with optional components.
* Concurrent from the ground up.
* Easy to use and reason about.
* Stingy with resources and efficient in execution.
* Designed for the age of agents: This was originally intended to be the core promise, and the initial architectural design was created specifically for this purpose. I later decided to take a "slow" approach. The term "agent" is not limited to ML-based agents (LLMs), but also includes **semantic** control agents.

> Note:
> - No stable data model. It is subject to major changes without prior notice. 
> - May periodically have compromised and/or incomplete implementations that are not safe to use except in test environments.
> - Many components are not public yet or partially public.
