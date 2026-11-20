-- Default permissions and system roles.

INSERT INTO permissions (code, description) VALUES
    ('organizations.read',   'View the current organization'),
    ('organizations.update', 'Update the current organization'),
    ('users.read',           'View users in the organization'),
    ('users.update',         'Update users in the organization'),
    ('users.suspend',        'Suspend or reactivate users'),
    ('invitations.read',     'View invitations'),
    ('invitations.create',   'Create invitations'),
    ('invitations.revoke',   'Revoke invitations'),
    ('roles.read',           'View roles and permissions'),
    ('roles.assign',         'Assign roles to users'),
    ('sessions.read',        'View sessions'),
    ('sessions.revoke',      'Revoke sessions'),
    ('audit.read',           'Read the audit log');

INSERT INTO roles (code, name, description, is_system) VALUES
    ('admin',      'Admin',      'Full control of the organization', TRUE),
    ('manager',    'Manager',    'Manages people, invites and sessions', TRUE),
    ('technician', 'Technician', 'Read-only access to people', TRUE),
    ('viewer',     'Viewer',     'Read-only access', TRUE);

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON TRUE
WHERE r.code = 'admin';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'organizations.read',
    'users.read',
    'users.update',
    'invitations.read',
    'invitations.create',
    'invitations.revoke',
    'roles.read',
    'roles.assign',
    'sessions.read',
    'sessions.revoke',
    'audit.read'
)
WHERE r.code = 'manager';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('organizations.read', 'users.read')
WHERE r.code = 'technician';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('organizations.read', 'users.read')
WHERE r.code = 'viewer';
