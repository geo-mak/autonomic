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

> Note:
> - No stable data model. It is subject to major changes without prior notice. 
> - Many components are not public yet or partially public.
> - The current implementation "tactically" uses a set of dependencies as scaffolding, but ultimately many of these will be replaced by custom solutions and internal protocols to minimize indirections and reduce the levels of abstraction that naturally reduce runtime efficiency.

### Implementation Highlights:
Autonomic consists of multiple components that can be used to build unified control planes.

The related components are organized into logical packages, each grouping together functionality that serves a specific purpose 
within the control plane.

The system exposes a public control API defined by a traits for interacting with its components, leaving the implementation 
details of it to the concrete implementations of API services.

Components have their own internal model of compositionality. Many components may run as separate processes, communicating via an internal IPC protocol. This internal model is not part of the public API, so changes to it do not affect external systems or 
consumers of the public interfaces.

Meanwhile, I have implemented an OpenAPI service with router, server with TLS support and client components, which was originally intended for testing since it is the easiest to change.
The gRPC service implementation is considered, but it is not a priority.

Areas of further work:
* Evolving the core design and the compositionality model.
* CLI and management tools for the client.
