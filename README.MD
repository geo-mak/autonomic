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

### Implementation Highlights:
Autonomic consists of multiple crates that can be used to build unified control planes.

### Core Crate
The core crate has the following main components:

* Operation: Operations are the core abstractions that define **concurrent** and **independent** **execution contexts**
with metadata and execution logic.<br>
Operations are defined by implementing the `Operation` trait, and they support **dynamic runtime parameters** and can <br>
be activated with **different values** each time.<br>
It is up to the operation to decide how to handle the parameters, which can be **ignored** or **rejected** with an <br>
error message.<br><br>

* Controller: An API that manages access to operations on its node. It acts as a registry and namespace for a <br>
set of operations.<br> When an operation is submitted to the controller, it is assigned a container instance.<br><br>

* Container: Encapsulates the operation and controls its associated components.<br>
Containers are private and managed internally by the system.<br><br>

* Effector: The controlling wrapper around operations inside the container, and it is responsible for the activation <br>
and state management of operations. Effectors are private and managed internally by the system.<br><br>

* Sensor: An observer that encapsulates an activation condition.<br>
Activation conditions are types that determine when an operation should be activated, and they are defined by <br>
implementing the `ActivationCondition` trait, which allow unconstrained definition of conditions.<br><br>

* Service: traits that define the control APIs (syscalls) of various components.<br><br>

* Data: Lightweight persistence component that defines standard APIs to persist and query data.<br>
**Note**: Data component is not public yet.<br><br>

* Tracing: Types and macros to support structured tracing of events.<br><br>

### Other Crates:

The current public crates are:

* Service: Concrete implementations of services and other components.<br>
Meanwhile, I have implemented an OpenAPI service with router, server with TLS support and client <br>
components, which was originally intended for testing since it is the easiest to change.<br>
The gRPC service implementation is considered, but it is not a priority.<br><br>

* Events: Tracing and monitoring APIs that allow systematic storage and retrieval of events with a set of layers and <br>
services.<br><br>

* Operations (only **demonstrative** at this stage): Pre-defined operations for common use-cases.<br><br>

* Conditions (only **demonstrative** at this stage): Pre-defined set of conditions for common use-cases.<br><br>

Areas of further work:
* Evolving the core design.
* CLI and management tools for the client.
* Data component.
