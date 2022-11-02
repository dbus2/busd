use enumflags2::BitFlags;
use parking_lot::RwLock;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use zbus::{
    fdo::{ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::{OwnedUniqueName, OwnedWellKnownName, UniqueName, WellKnownName},
};

#[derive(Clone, Debug)]
pub struct NameRegistry {
    names: Arc<RwLock<HashMap<OwnedWellKnownName, NameEntry>>>,
}

#[derive(Clone, Debug)]
pub struct NameEntry {
    owner: NameOwner,
    waiting_list: VecDeque<NameOwner>,
}

#[derive(Clone, Debug)]
pub struct NameOwner {
    unique_name: Arc<OwnedUniqueName>,
    allow_replacement: bool,
}

impl NameRegistry {
    pub fn new() -> Self {
        Self {
            names: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn request_name(
        &self,
        name: OwnedWellKnownName,
        unique_name: Arc<OwnedUniqueName>,
        flags: BitFlags<RequestNameFlags>,
    ) -> RequestNameReply {
        // TODO: Emit all signals.
        let owner = NameOwner {
            unique_name,
            allow_replacement: flags.contains(RequestNameFlags::AllowReplacement),
        };
        let mut names = self.names.write();

        match names.get_mut(&name) {
            Some(entry) => {
                if entry.owner.unique_name == owner.unique_name {
                    RequestNameReply::AlreadyOwner
                } else if flags.contains(RequestNameFlags::ReplaceExisting)
                    && entry.owner.allow_replacement
                {
                    entry.owner = owner;

                    RequestNameReply::PrimaryOwner
                } else if !flags.contains(RequestNameFlags::DoNotQueue) {
                    entry.waiting_list.push_back(owner);

                    RequestNameReply::InQueue
                } else {
                    RequestNameReply::Exists
                }
            }
            None => {
                names.insert(
                    name,
                    NameEntry {
                        owner,
                        waiting_list: VecDeque::new(),
                    },
                );

                RequestNameReply::PrimaryOwner
            }
        }
    }

    pub fn release_name(&self, name: WellKnownName, owner: UniqueName) -> ReleaseNameReply {
        // TODO: Emit all signals.
        let mut names = self.names.write();
        match names.get_mut(name.as_str()) {
            Some(entry) => {
                if *entry.owner.unique_name == owner {
                    if let Some(owner) = entry.waiting_list.pop_front() {
                        entry.owner = owner;
                    } else {
                        names.remove(name.as_str());
                    }

                    ReleaseNameReply::Released
                } else {
                    for (i, waiting) in entry.waiting_list.iter().enumerate() {
                        if *waiting.unique_name == owner {
                            entry.waiting_list.remove(i);

                            return ReleaseNameReply::Released;
                        }
                    }

                    ReleaseNameReply::NonExistent
                }
            }
            None => ReleaseNameReply::NonExistent,
        }
    }

    pub fn lookup(&self, name: WellKnownName) -> Option<Arc<OwnedUniqueName>> {
        self.names
            .read()
            .get(name.as_str())
            .map(|e| e.owner.unique_name.clone())
    }
}
