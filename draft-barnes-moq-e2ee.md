---
title: "End-to-End Security for Media over QUIC"
abbrev: "MOQ E2EE"
category: info

docname: draft-barnes-moq-e2ee-latest
submissiontype: IETF  # also: "independent", "editorial", "IAB", or "IRTF"
number:
date:
consensus: true
v: 3
area: ""
workgroup: "Media Over QUIC"
keyword:
 - end-to-end
 - media over quic
venue:
  group: "Media Over QUIC"
  type: ""
  mail: "moq@ietf.org"
  arch: "https://mailarchive.ietf.org/arch/browse/moq/"
  github: "bifurcation/moq-e2ee"

author:
 -
    fullname: Richard L. Barnes
    organization: Cisco
    email: rlb@ipv.sx
 -
    fullname: Suhas Nandakumar
    organization: Cisco
    email: snandaku@cisco.com

normative:

informative:


--- abstract

Media Over QUIC Transport (MOQT) provides a simple protocol for distributing
media objects over a network of relays.  The content of MoQ objects is supposed
to be opaque to relays.  However, the base MoQ protocol does not assure this
property cryptographically.  This document defines a scheme for authorized
endpoints in a MoQ session to establish keys that are not accessible to relays,
and to use those keys to encrypt MoQ objects so that relays cannot examine their
content. 

--- middle

# Introduction

TODO Introduction

# Conventions and Definitions

{::boilerplate bcp14-tagged}

# Protocol Overview

First member joins:

```
A->Origin: GET catalog
Origin->A: catalog, incl. DS URL, welcome_namespace, group_namespace
A->Relay: SubscribeRequest(welcome_namespace)
A->Relay: SubscribeRequest(group_namespace)
A->DS: POST /join -> key_package
DS->A: 201 Created
A: Create MLS group
A->Relay: SubscribeEnd(welcome_namespace)
A->Relay: PublishRequest(anything A wants to publish)
```

Second member joins:

```
B->Origin: GET catalog
Origin->B: catalog, incl. DS URL, welcome_namespace, request_namespace
B->Relay: SubscribeRequest(welcome_namespace)
B->Relay: SubscribeRequest(group_namespace)
B->DS: POST /join -> key_package
DS->B: 202 Accepted
DS->group_namespace: JoinRequest(key_package)
B->DS: Commit(Add(key_package))
```

Third member C joins in the same way.  A and B run a local algorithm to 
decide who will try to commit first, with DS acting as tie breaker.  To
attempt a commit, a member POSTs to the DS.  If it's accepted, great; if
it's rejected as stale, get the latest Commit and retry.  DS sends Commit
out under `group_namespace`.

B disconnects from the session:

```
B->DS: POST /leave -> remove_proposal
DS->B: 202 Accepted
DS->group_namespace: LeaveRequest(remove_proposal)
C->DS: Commit(Add(key_package))
```

[[ We probably want to make Light MLS a part of this, so that broadcast-like
sessions can be supported cheaply.  For example, subscribers could be light
and publishers could be full clients. ]]

[[ Or we could have a "Zoom-like" mode where you connect to some trusted key
distributor and just get a key from them.  (Presumably with 1:1 MLS.)  But
that has some other complexities, e.g., who is the KD?  Maybe this could be
emulated with the same API? ]]

# Additional Catalog Information

* [[ URL, namespaces in catalog ]]

# Key Establishment

```
welcome_namespace
group_namespace
POST /join
POST /leave
POST /commit
```

* Prospective joiners send KeyPackage to /join 
* Prospective leavers send Remove proposal to /leave 
* ... possibly also DS to clean up stale participants
* DS internal state: Basically just current epoch
* How does the ratchet tree get distributed?
    * Included in Welcome
    * HTTP download from DS (masked?)
    * Some scheme for distributing it over MoQ

# Object Encryption

* [[ SFrame-like ]]

# Identity

* [[ Anonymous; just use public keys ]]
* [[ Really whatever scheme the clients support, the servers don't care ]]


# Security Considerations

TODO Security


# IANA Considerations

This document has no IANA actions.


--- back

# Acknowledgments
{:numbered="false"}

TODO acknowledge.
