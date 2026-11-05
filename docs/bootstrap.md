# Bootstrap a cloud backend

Prepare a private Pulumi state bucket on AWS S3 or DigitalOcean Spaces. Repeated runs reuse
resources belonging to the same deployment. Run locally or through the manually triggered
**Bootstrap cloud backend** workflow. No service is installed on the application server.
Needs Node.js/npm; wrapping Pulumi also needs its CLI.

## Start in GitHub Actions

1. Create a GitHub environment under **Settings → Environments**.
2. Add the required variables below and configure authentication for the chosen provider.
3. Run **Actions → Bootstrap cloud backend → Run workflow**, selecting that environment.

The workflow creates the backend bucket and, for DigitalOcean, finds or creates its project.
It reports the backend URL and project ID in the job summary. It does not create a VM or deploy apps.
Ordinary CI and pull requests run tests without cloud credentials. Keep real names, domains,
IDs, and credentials in environment settings or ignored local files; the workflow stays reusable.

## Settings

| Variable | Required | Meaning |
| --- | --- | --- |
| `ROOTSTOCK_CLOUD` | Yes | `aws` or `digitalocean` |
| `ROOTSTOCK_NAMESPACE` | Yes | Stable deployment identity: 2–63 lowercase letters, digits, or hyphens, starting and ending with a letter or digit |
| `ROOTSTOCK_STATE_BUCKET` | Yes | Globally unique bucket name: 3–63 lowercase letters, digits, or hyphens, starting and ending with a letter or digit |
| `ROOTSTOCK_STATE_REGION` | Yes | Bucket region, such as `us-east-1` for AWS or `nyc3` for Spaces; independent of the VM region |
| `ROOTSTOCK_PROJECT_ID` | DigitalOcean: optional | Existing project UUID; otherwise match the namespace exactly or create a project with that name |
| `ROOTSTOCK_ADOPT_STATE_BUCKET` | No | `true` explicitly adopts an unmarked private bucket; defaults to `false`; remove after adoption |
| `AWS_ROLE_ARN` | AWS workflow | Role assumed through GitHub OIDC; local commands use the standard AWS SDK credential chain |
| `DIGITALOCEAN_TOKEN` | DigitalOcean secret | API token used to manage the project and temporary Spaces keys |

Project names must match unambiguously; provide an ID if multiple projects share a name.
The bucket's `.rootstock/bootstrap.json` records its account, provider, region, namespace, and
DigitalOcean project. Mismatches fail rather than overwrite ownership. Access-denied errors never
trigger resource creation. Explicit adoption does not override another deployment's record.

### DigitalOcean authentication

Store `DIGITALOCEAN_TOKEN` as an environment secret. Bootstrap needs:

- `project:read`, `project:assign_resource`; also `project:create` when creating a project.
- `spaces_key:read`, `spaces_key:create_credentials`, `spaces_key:delete`.
- Required dependencies: `spaces:read`, `regions:read`, `sizes:read`, `actions:read`.

See [token scopes](https://docs.digitalocean.com/reference/api/scopes/). Host provisioning needs
its own resource permissions as described in the [configuration reference](configuration.md#optional-cloud-infrastructure).
Projects organize resources; token permissions are not isolated by project.

For bootstrap and the full Pulumi host lifecycle in an **existing project**, use this profile:

| Resource | Scopes |
| --- | --- |
| `droplet` | `create`, `read`, `update`, `delete` |
| `firewall` | `create`, `read`, `update`, `delete` |
| `reserved_ip` | `create`, `read`, `update`, `delete` |
| `spaces_key` | `read`, `create_credentials`, `delete` |
| `project` | `read`, `assign_resource` |
| `actions`, `regions`, `sizes`, `image`, `snapshot`, `spaces`, `ssh_key`, `vpc` | `read` |

The project response supplies its owner UUID, so bootstrap needs no `account:read` scope.
This profile uses an existing SSH key and excludes managed databases, DNS changes, project creation,
and Spaces-key updates. Add the initial DNS A record through the console. Separate DNS automation
needs `domain:read`, `domain:create`, and `domain:update` for creating and updating records.

Bootstrap creates a temporary full-access Spaces key to prepare and inspect the bucket, then
revokes it. A wrapped command receives a separate key scoped to that bucket. Keys are masked in
Actions logs and revoked after success or failure; their secrets are never returned as outputs.
Keys have no automatic expiry: cancellation or runner loss before cleanup can leave keys behind.
Remove orphaned keys named `rootstock-<namespace>-...` in the Spaces console before retrying.

Spaces state buckets must have private ACLs and no bucket policy, matching the scoped-key model.
The S3 signing region is `us-east-1`; the endpoint uses `ROOTSTOCK_STATE_REGION`.

### AWS authentication

Configure a GitHub OIDC provider and role following the
[official credentials action guide](https://github.com/aws-actions/configure-aws-credentials#oidc-configuration).
Restrict its trust to your repository and selected environment. Set the role ARN as `AWS_ROLE_ARN`.

The role needs `s3:CreateBucket`, `s3:ListBucket`, `s3:GetBucketAcl`, and
`s3:PutBucketPublicAccessBlock` on the chosen bucket, plus `s3:GetObject` and `s3:PutObject` on its
`.rootstock/bootstrap.json` object. The SDK verifies bucket ownership against the authenticated
account and enables all S3 public-access blocks. The bootstrap command never deletes the bucket.

For Pulumi commands, also permit reading, writing, listing, and deleting state and lock objects
under `pulumi-state/`. See [Pulumi S3 permissions](https://www.pulumi.com/docs/iac/operations/stack-management/using-a-diy-backend/#required-permissions).

## Use locally or wrap Pulumi

From the repository root:

```sh
cd infra
npm ci
cp .env.example .env
```

Edit the ignored `.env` with your settings. Inject credentials from your password manager or AWS
profile; keep secret values out of shell arguments and source files. Then load the ordinary settings:

```sh
set -a
. ./.env
set +a
npm run bootstrap
```

The command prints JSON containing `backendUrl` and, for DigitalOcean, `projectId`.
It also writes `backend_url` and `project_id` when GitHub supplies `GITHUB_OUTPUT`.
These outputs contain deployment identifiers; they do not contain credentials.

For authenticated Pulumi access, pass the command as arguments. Bootstrap supplies
`PULUMI_BACKEND_URL` and the appropriate credentials only to that child process:

```sh
npm run bootstrap -- pulumi stack select prod --create --secrets-provider passphrase
npm run bootstrap -- pulumi preview --stack prod
npm run bootstrap -- pulumi up --stack prod
```

Provide `PULUMI_CONFIG_PASSPHRASE` securely for encrypted stacks, with a recoverable copy outside
the backend. Keep your `Pulumi.<stack>.yaml` ignored. DigitalOcean children also receive
`ROOTSTOCK_PROJECT_ID`; set the stack's `do:projectId` to that value when configuring a new stack.
CI provisioning needs its own SSH authentication; it cannot use a workstation's Keychain.

The state bucket exists before Pulumi starts and remains outside the application stack's lifecycle.
Destroying that stack does not remove its backend. Bucket ownership does not import existing VMs
into Pulumi: adopt those explicitly and review the resulting preview.

Only one bootstrap should write a backend at a time. The supplied workflow serializes runs per
GitHub environment. Use the same deployment concurrency group in workflows that wrap this command.
Cloud account access, globally unique names, and authenticated provisioning remain live checks;
the automated suite exercises SDK calls and API boundaries without creating cloud resources.
