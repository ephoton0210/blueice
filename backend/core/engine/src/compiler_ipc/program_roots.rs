// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Multi-root registration exists only on the trusted startup owner.

use super::*;

impl CoreCompilerProjectCatalog {
    /// Selects one immutable root program with explicit query visibility.
    pub fn register_startup_program(
        &mut self,
        registration: RegisteredProjectRegistration,
        roots: Vec<String>,
    ) -> Result<CompilerProject, CompilerServiceError> {
        self.register_program_visibility(registration, roots, true)
    }

    /// A private program receives neither query visibility nor output authority.
    pub fn register_startup_program_private(
        &mut self,
        registration: RegisteredProjectRegistration,
        roots: Vec<String>,
    ) -> Result<CompilerProject, CompilerServiceError> {
        self.register_program_visibility(registration, roots, false)
    }

    fn register_program_visibility(
        &mut self,
        registration: RegisteredProjectRegistration,
        roots: Vec<String>,
        exposed: bool,
    ) -> Result<CompilerProject, CompilerServiceError> {
        let project =
            self.adapter
                .register_core_program_with_visibility(registration, roots, exposed)?;
        self.registered_projects.push(project);
        Ok(project)
    }

    /// Grants physical publication separately from the immutable root selection.
    pub fn register_startup_program_with_output_write_grant(
        &mut self,
        registration: RegisteredProjectRegistration,
        roots: Vec<String>,
    ) -> Result<CompilerProject, OwnerOutputWriteGrantError> {
        self.register_program_output(registration, roots, true)
    }

    /// A private output grant cannot promote the program into query inventories.
    pub fn register_startup_program_private_with_output_write_grant(
        &mut self,
        registration: RegisteredProjectRegistration,
        roots: Vec<String>,
    ) -> Result<CompilerProject, OwnerOutputWriteGrantError> {
        self.register_program_output(registration, roots, false)
    }

    fn register_program_output(
        &mut self,
        registration: RegisteredProjectRegistration,
        roots: Vec<String>,
        exposed: bool,
    ) -> Result<CompilerProject, OwnerOutputWriteGrantError> {
        let grant = OwnerOutputWriteGrant::for_registration(&registration)
            .map_err(OwnerOutputWriteGrantError::InvalidPhysicalRoot)?;
        let project = self
            .register_program_visibility(registration, roots, exposed)
            .map_err(OwnerOutputWriteGrantError::Registration)?;
        let id =
            RegisteredProjectId::from_wire(project.id).expect("registered program has an owner ID");
        self.output_write_grants.insert(id, grant);
        Ok(project)
    }
}
