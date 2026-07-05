import * as pulumi from "@pulumi/pulumi";
import * as hcloud from "@pulumi/hcloud";
import * as command from '@pulumi/command';
import * as k8s from "@pulumi/kubernetes";
import * as cloudflare from "@pulumi/cloudflare";
import * as nginx from "@pulumi/kubernetes-ingress-nginx";
import * as aws from "@pulumi/aws";

const config = new pulumi.Config();
const subdomainPrefix = pulumi.getStack();
const baseDomain = `${subdomainPrefix}.get-agon.com`;

// pulumi config set --secret privateKeyBase64 "$(cat ~/.ssh/pulumi_agon_key | base64)"
const privateKeyBase64 = config.requireSecret("privateKeyBase64");
const privateKey = privateKeyBase64.apply(key => Buffer.from(key, 'base64').toString('utf8'));

// pulumi config set --secret publicKey "$(cat ~/.ssh/pulumi_agon_key.pub | tr -d '\n\r')"
const publicKey = config.requireSecret("publicKey");

const sshKey = new hcloud.SshKey("main", {
	name: `${pulumi.getStack()}-ssh-key`,
	publicKey: publicKey,
});

const node = new hcloud.Server("node", {
	name: `${pulumi.getStack()}-node`,
	image: "ubuntu-26.04",
	serverType: "cpx22",
	publicNets: [{
		ipv4Enabled: true,
		ipv6Enabled: true,
	}],
	sshKeys: [sshKey.name],
});

const cloudflareZoneId = '3d1b2636aa31acc40c4044830fe4982c';

const dnsRecord = new cloudflare.DnsRecord('record', {
	zoneId: cloudflareZoneId,
	name: `*.${baseDomain}`,
	type: 'A',
	content: node.ipv4Address,
	ttl: 300,
	proxied: false,
});

const connection = {
	host: node.ipv4Address,
	user: 'root',
	privateKey,
};

const installK3s = new command.remote.Command('install-k3s', {
	connection,
	create: 'curl -sfL https://get.k3s.io | sh -s - --disable=traefik',
})

const getKubeconfig = new command.remote.Command('get-kubeconfig', {
	connection,
	create: 'cat /etc/rancher/k3s/k3s.yaml',
}, { dependsOn: [installK3s] })

const kubeconfig = pulumi.all([getKubeconfig.stdout, node.ipv4Address]).apply(([kubeconfig, serverIp]) => {
	return kubeconfig.replace('127.0.0.1', serverIp)
});

const k8sProvider = new k8s.Provider('k3s-provider', {
	kubeconfig,
})

const cloudnativePgNamespace = new k8s.core.v1.Namespace("cloudnative-pg-namespace", {
	metadata: {
		name: "cnpg-system",
	}
}, { provider: k8sProvider });

const cloudnativePg = new k8s.helm.v4.Chart('cloudnative-pg', {
	chart: 'cloudnative-pg',
	version: 'v0.24.0',
	repositoryOpts: {
		repo: 'https://cloudnative-pg.github.io/charts'
	},
	namespace: cloudnativePgNamespace.metadata.apply(metadata => metadata.name),
}, { provider: k8sProvider })

const ctrl = new nginx.IngressController("nginx-ingress", {
	controller: {
		publishService: {
			enabled: true,
		},
	},
}, { provider: k8sProvider });

const certManagerNamespace = new k8s.core.v1.Namespace("cert-manager-namespace", {
	metadata: {
		name: "cert-manager",
	}
}, { provider: k8sProvider });

const certManager = new k8s.helm.v4.Chart('cert-manager', {
	chart: 'cert-manager',
	version: 'v1.17.2',
	repositoryOpts: {
		repo: 'https://charts.jetstack.io'
	},
	namespace: certManagerNamespace.metadata.apply(metadata => metadata.name),
	values: {
		crds: {
			enabled: true,
		}
	}
}, { provider: k8sProvider })

const issuer = new k8s.apiextensions.CustomResource('letsencrypt-prod', {
	apiVersion: 'cert-manager.io/v1',
	kind: 'ClusterIssuer',
	metadata: {
		name: 'letsencrypt-production',
		namespace: 'default',
	},
	spec: {
		acme: {
			email: config.get('letsencrypt-email'),
			server: 'https://acme-v02.api.letsencrypt.org/directory',
			privateKeySecretRef: {
				name: 'letsencrypt-production'
			},
			solvers: [
				{
					http01: {
						ingress: {
							class: 'nginx',
						}
					}
				}
			]
		}
	},
}, { provider: k8sProvider, dependsOn: [certManager, ctrl] })

// https://github.com/temporalio/helm-charts#install-with-your-own-postgresql
// helm install \
//   --repo https://go.temporal.io/helm-charts \
//   -f values/values.postgresql.yaml \
//   --set elasticsearch.enabled=false \
//   --set server.config.persistence.default.sql.user=postgresql_user \
//   --set server.config.persistence.default.sql.password=postgresql_password \
//   --set server.config.persistence.visibility.sql.user=postgresql_user \
//   --set server.config.persistence.visibility.sql.password=postgresql_password \
//   --set server.config.persistence.default.sql.host=postgresql_host \
//   --set server.config.persistence.visibility.sql.host=postgresql_host \
//   temporaltest temporal --timeout 900s

const temporalDb = new k8s.apiextensions.CustomResource("temporal-db", {
	apiVersion: "postgresql.cnpg.io/v1",
	kind: "Cluster",
	metadata: {
		name: "temporal",
	},
	spec: {
		instances: 1,
		storage: {
			size: "1Gi",
		},
		monitoring: {
			enablePodMonitor: true,
		},
	},
}, { provider: k8sProvider, dependsOn: [cloudnativePg] });

const temporalVisibilityDb = new k8s.apiextensions.CustomResource("temporal-visibility-db", {
	apiVersion: "postgresql.cnpg.io/v1",
	kind: "Cluster",
	metadata: {
		name: "temporal-visibility",
	},
	spec: {
		instances: 1,
		storage: {
			size: "1Gi",
		},
		monitoring: {
			enablePodMonitor: true,
		},
	},
}, { provider: k8sProvider, dependsOn: [cloudnativePg] });

const temporalDbSecretName = temporalDb.metadata.name.apply(value => `${value}-app`)
const temporalVisibilityDbSecretName = temporalVisibilityDb.metadata.name.apply(value => `${value}-app`)
// 
// const temporalDatabaseSecretData = k8s.core.v1.Secret.get(
// 	'temporal-db-secret', temporalDbSecretName.apply(value => `default/${value}`),
// 	{ dependsOn: [temporalDb], provider: k8sProvider }
// ).data
// 
// const temporalVisibilityDatabaseSecretData = k8s.core.v1.Secret.get(
// 	'temporal-visibility-db-secret', temporalVisibilityDbSecretName.apply(value => `default/${value}`),
// 	{ dependsOn: [temporalVisibilityDb], provider: k8sProvider }
// ).data

// const temporalDatabaseConnectionDetails = pulumi.all([temporalDbSecretName, temporalDatabaseSecretData]).apply(([name, data]) => {
// 	return {
// 		host: data['hostname'],
// 		user: data['username'],
// 		existingSecret: name,
// 		database: data['dbname'],
// 	};
// })
// 
// const temporalVisibilityConnectionDetails = pulumi.all([temporalVisibilityDbSecretName, temporalVisibilityDatabaseSecretData]).apply(([name, data]) => {
// 	return {
// 		host: data['hostname'],
// 		user: data['username'],
// 		existingSecret: name,
// 		database: data['dbname'],
// 	};
// })

const temporalDbConnectionConfig = {
	maxConns: 20,
	maxIdleConns: 20,
	maxConnLifetime: "1h",
	driver: 'postgres12',
};

const dbConnectionDetails = {
	host: "temporal-rw",
	port: 5432,
	database: "app",
	user: "app",
	existingSecret: "temporal-app",
}

const visibilityDbConnectionDetails = {
	host: "temporal-visibility-rw",
	port: 5432,
	database: "app",
	user: "app",
	existingSecret: "temporal-visibility-app",
}

// const temporalHelmValues = pulumi.all([temporalDatabaseConnectionDetails, temporalVisibilityConnectionDetails]).apply(([dbConnectionDetails, visibilityDbConnectionDetails]) => {
const temporalHelmValues = {
	server: {
		config: {
			persistence: {
				default: {
					driver: "sql",
					sql: {
						...temporalDbConnectionConfig,
						...dbConnectionDetails,
					},
				},
				visibility: {
					driver: "sql",
					sql: {
						...temporalDbConnectionConfig,
						...visibilityDbConnectionDetails,
					},
				},
			},
		}
	},
	schema: {
		createDatabase: {
			enabled: false,
		},
		setup: {
			enabled: true,
		},
		update: {
			enabled: true,
		},
	},
	postgresql: {
		enabled: true,
	},
	elasticsearch: {
		enabled: false,
	},
	mysql: {
		enabled: false,
	},
	prometheus: {
		enabled: false,
	},
	grafana: {
		enabled: false,
	},
	cassandra: {
		enabled: false,
	},
};

const temporal = new k8s.helm.v4.Chart('temporal', {
	chart: 'temporal',
	version: 'v0.63.0',
	repositoryOpts: {
		repo: 'https://go.temporal.io/helm-charts'
	},
	values: temporalHelmValues,
}, { provider: k8sProvider })

// ───────────────────────────────────────────────────────────────────────────
// AWS: DynamoDB single-table + least-privilege app credentials
// See docs/dynamodb-design.md. All entities live in one table addressed by
// PK/SK, with three overloaded GSIs. Streams are enabled to feed the async
// pipeline (EventBridge Pipe → SQS → worker; see docs/async-design.md).
// ───────────────────────────────────────────────────────────────────────────

// AWS credentials + region are read from the `aws:` config namespace by the
// default provider, exactly like cloudflare:apiToken / hcloud:token. Set them
// as stack secrets (see the commented commands at the top of this file group):
//   pulumi config set aws:region eu-west-1
//   pulumi config set --secret aws:accessKey <id>
//   pulumi config set --secret aws:secretKey <secret>
// The region is also needed as a plain value for the app's env below.
const awsRegion = new pulumi.Config("aws").require("region");

const dynamoTable = new aws.dynamodb.Table("agon", {
	name: `agon-${pulumi.getStack()}`,
	billingMode: "PAY_PER_REQUEST",
	hashKey: "PK",
	rangeKey: "SK",
	// Only key/index attributes are declared; all others are schemaless.
	attributes: [
		{ name: "PK", type: "S" },
		{ name: "SK", type: "S" },
		{ name: "GSI1PK", type: "S" },
		{ name: "GSI1SK", type: "S" },
		{ name: "GSI2PK", type: "S" },
		{ name: "GSI2SK", type: "S" },
		{ name: "GSI3PK", type: "S" },
		{ name: "GSI3SK", type: "S" },
	],
	globalSecondaryIndexes: [
		{ name: "GSI1", hashKey: "GSI1PK", rangeKey: "GSI1SK", projectionType: "ALL" },
		{ name: "GSI2", hashKey: "GSI2PK", rangeKey: "GSI2SK", projectionType: "ALL" },
		{ name: "GSI3", hashKey: "GSI3PK", rangeKey: "GSI3SK", projectionType: "ALL" },
	],
	// Streams feed the async fan-out / search-indexing pipeline.
	streamEnabled: true,
	streamViewType: "NEW_AND_OLD_IMAGES",
	// Optional auto-expiry for ephemeral items (e.g. pending assets); items opt
	// in by setting a numeric `ttl` attribute (epoch seconds).
	ttl: { attributeName: "ttl", enabled: true },
	pointInTimeRecovery: { enabled: true },
});

// Least-privilege IAM user for the API/worker: CRUD on the table + query on its
// indexes + read the stream. No table-management or account-wide permissions.
const appAwsUser = new aws.iam.User("agon-app", {
	name: `agon-app-${pulumi.getStack()}`,
});

new aws.iam.UserPolicy("agon-app-dynamodb", {
	user: appAwsUser.name,
	policy: pulumi.jsonStringify({
		Version: "2012-10-17",
		Statement: [
			{
				Sid: "TableCrud",
				Effect: "Allow",
				Action: [
					"dynamodb:GetItem",
					"dynamodb:BatchGetItem",
					"dynamodb:PutItem",
					"dynamodb:UpdateItem",
					"dynamodb:DeleteItem",
					"dynamodb:Query",
					"dynamodb:TransactWriteItems",
					"dynamodb:TransactGetItems",
					"dynamodb:ConditionCheckItem",
				],
				// Table and all its indexes.
				Resource: [dynamoTable.arn, pulumi.interpolate`${dynamoTable.arn}/index/*`],
			},
			{
				Sid: "ReadStream",
				Effect: "Allow",
				Action: [
					"dynamodb:GetRecords",
					"dynamodb:GetShardIterator",
					"dynamodb:DescribeStream",
					"dynamodb:ListStreams",
				],
				Resource: [pulumi.interpolate`${dynamoTable.arn}/stream/*`],
			},
		],
	}),
});

// ── S3: asset storage ───────────────────────────────────────────────────────
// Private bucket for user-uploaded images (profile / team / match). The app
// hands clients presigned PUT URLs to upload and presigned GET URLs to read, so
// the bucket stays private and the API mediates all access. Provider-agnostic
// by design (see the Asset API) — swappable for R2/MinIO later without app
// changes.
const assetsBucket = new aws.s3.BucketV2("agon-assets", {
	bucket: `agon-assets-${pulumi.getStack()}`,
});

// Lock down public access; objects are only reachable via presigned URLs.
new aws.s3.BucketPublicAccessBlock("agon-assets", {
	bucket: assetsBucket.id,
	blockPublicAcls: true,
	blockPublicPolicy: true,
	ignorePublicAcls: true,
	restrictPublicBuckets: true,
});

// Emit S3 events to EventBridge; the async pipeline (S3 → EventBridge → SQS →
// worker) uses "object created" to flip a Pending asset to Uploaded. The rule
// itself is added with the async infra pass (worker/queue don't exist yet).
new aws.s3.BucketNotification("agon-assets", {
	bucket: assetsBucket.id,
	eventbridge: true,
});

// Expire incomplete multipart uploads so abandoned uploads don't linger.
new aws.s3.BucketLifecycleConfigurationV2("agon-assets", {
	bucket: assetsBucket.id,
	rules: [
		{
			id: "abort-incomplete-uploads",
			status: "Enabled",
			abortIncompleteMultipartUpload: { daysAfterInitiation: 7 },
		},
	],
});

new aws.iam.UserPolicy("agon-app-s3", {
	user: appAwsUser.name,
	policy: pulumi.jsonStringify({
		Version: "2012-10-17",
		Statement: [
			{
				Sid: "AssetObjectRw",
				Effect: "Allow",
				Action: ["s3:PutObject", "s3:GetObject"],
				Resource: [pulumi.interpolate`${assetsBucket.arn}/*`],
			},
		],
	}),
});

// ───────────────────────────────────────────────────────────────────────────
// Async pipeline: DynamoDB Streams → EventBridge Pipe → SQS → agon_worker
// Every committed write is captured off the table's stream and delivered to a
// durable SQS queue that the self-hosted worker long-polls. This is the
// at-least-once guarantee (see docs/async-design.md §2/§3). The Pipe transforms
// each raw stream record into the thin `{event, pk, sk}` envelope the worker
// consumes, so the worker never sees the raw stream wire format (§12.3).
// ───────────────────────────────────────────────────────────────────────────

// Dead-letter queue for messages that repeatedly fail processing, so a poison
// message can't block the main queue forever.
const eventsDlq = new aws.sqs.Queue("agon-events-dlq", {
	name: `agon-events-dlq-${pulumi.getStack()}`,
	messageRetentionSeconds: 1209600, // 14 days — max, for inspection.
});

// The main events queue the worker long-polls. Visibility timeout matches the
// worker's per-message processing budget; failures redeliver after it elapses.
const eventsQueue = new aws.sqs.Queue("agon-events", {
	name: `agon-events-${pulumi.getStack()}`,
	visibilityTimeoutSeconds: 60,
	messageRetentionSeconds: 345600, // 4 days.
	redrivePolicy: pulumi.jsonStringify({
		deadLetterTargetArn: eventsDlq.arn,
		maxReceiveCount: 5,
	}),
});

// The worker needs to consume the queue (in addition to the DynamoDB perms it
// already has via the shared app user).
new aws.iam.UserPolicy("agon-app-sqs", {
	user: appAwsUser.name,
	policy: pulumi.jsonStringify({
		Version: "2012-10-17",
		Statement: [
			{
				Sid: "ConsumeEventsQueue",
				Effect: "Allow",
				Action: [
					"sqs:ReceiveMessage",
					"sqs:DeleteMessage",
					"sqs:GetQueueAttributes",
				],
				Resource: [eventsQueue.arn],
			},
		],
	}),
});

// Role assumed by the EventBridge Pipe: read the DynamoDB stream, write to SQS.
const pipeRole = new aws.iam.Role("agon-pipe-role", {
	name: `agon-pipe-${pulumi.getStack()}`,
	assumeRolePolicy: pulumi.jsonStringify({
		Version: "2012-10-17",
		Statement: [
			{
				Effect: "Allow",
				Principal: { Service: "pipes.amazonaws.com" },
				Action: "sts:AssumeRole",
			},
		],
	}),
});

new aws.iam.RolePolicy("agon-pipe-policy", {
	role: pipeRole.id,
	policy: pulumi.jsonStringify({
		Version: "2012-10-17",
		Statement: [
			{
				Sid: "ReadStream",
				Effect: "Allow",
				Action: [
					"dynamodb:DescribeStream",
					"dynamodb:GetRecords",
					"dynamodb:GetShardIterator",
					"dynamodb:ListStreams",
				],
				Resource: [dynamoTable.streamArn],
			},
			{
				Sid: "WriteQueue",
				Effect: "Allow",
				Action: ["sqs:SendMessage"],
				Resource: [eventsQueue.arn],
			},
		],
	}),
});

// The Pipe: stream source → SQS target, transforming each record into the
// worker's envelope. The `event`/`pk`/`sk` values are strings (quoted); the
// `old_image`/`new_image` values are whole DynamoDB item objects, so their
// placeholders are UNQUOTED (EventBridge emits the nested JSON object, and adds
// quotes only around string placeholders — see the input-transform docs). The
// table stream is NEW_AND_OLD_IMAGES, so both images are available.
//
// Absent paths are omitted from the output, so an INSERT (no OldImage) or a
// REMOVE (no NewImage) simply drops that field — the worker treats it as None.
// Images are in raw DynamoDB attribute-value shape ({"S":"..."} etc.); the
// worker uses them opportunistically (e.g. detecting a status transition) and
// otherwise re-reads current state from the table.
new aws.pipes.Pipe("agon-events-pipe", {
	name: `agon-events-${pulumi.getStack()}`,
	roleArn: pipeRole.arn,
	source: dynamoTable.streamArn,
	sourceParameters: {
		dynamodbStreamParameters: {
			startingPosition: "LATEST",
			batchSize: 10,
			maximumRetryAttempts: 3,
		},
	},
	target: eventsQueue.arn,
	targetParameters: {
		inputTemplate: '{"event": "<$.eventName>", "pk": "<$.dynamodb.Keys.PK.S>", "sk": "<$.dynamodb.Keys.SK.S>", "old_image": <$.dynamodb.OldImage>, "new_image": <$.dynamodb.NewImage>}',
	},
});

const appAwsAccessKey = new aws.iam.AccessKey("agon-app", {
	user: appAwsUser.name,
});

// AWS creds + table name + queue URL for the service and worker as a k8s
// secret, injected into the deployment envs below.
const awsSecret = new k8s.core.v1.Secret("aws-credentials", {
	metadata: { name: "aws-credentials", namespace: "default" },
	type: "Opaque",
	stringData: {
		AWS_ACCESS_KEY_ID: appAwsAccessKey.id,
		AWS_SECRET_ACCESS_KEY: appAwsAccessKey.secret,
		AWS_REGION: awsRegion,
		AGON_TABLE_NAME: dynamoTable.name,
		AGON_ASSETS_BUCKET: assetsBucket.bucket,
		AGON_EVENTS_QUEUE_URL: eventsQueue.url,
	},
}, { provider: k8sProvider });

// Create a secret for JWT
const jwtSecret = new k8s.core.v1.Secret("jwt-secret", {
	metadata: {
		name: "jwt-secret",
		namespace: "default",
	},
	type: "Opaque",
	data: {
		"jwt-secret": config.requireSecret("jwtSecret").apply(val => Buffer.from(val).toString("base64")),
	},
}, { provider: k8sProvider });

// ───────────────────────────────────────────────────────────────────────────
// Meilisearch: search / discovery index
// Self-hosted search engine backing the discovery endpoints (users / teams /
// matches). The async worker keeps its indexes in sync off the DynamoDB stream
// (see docs/async-design.md §7); the API queries it and hydrates from DynamoDB.
// In-cluster only — reached via the `meilisearch` Service, never exposed
// publicly (no ingress). State lives on a PVC so index data survives restarts.
// ───────────────────────────────────────────────────────────────────────────

// Master key, held as a k8s secret. Set it as a stack secret:
//   pulumi config set --secret meiliMasterKey <key>   (>= 16 bytes)
const meiliSecret = new k8s.core.v1.Secret("meili-master-key", {
	metadata: { name: "meili-master-key", namespace: "default" },
	type: "Opaque",
	stringData: {
		MEILI_MASTER_KEY: config.requireSecret("meiliMasterKey"),
	},
}, { provider: k8sProvider });

// Persistent storage for the Meilisearch data directory (/meili_data).
const meiliPvc = new k8s.core.v1.PersistentVolumeClaim("meili-data", {
	metadata: { name: "meili-data", namespace: "default" },
	spec: {
		accessModes: ["ReadWriteOnce"],
		resources: { requests: { storage: "2Gi" } },
	},
}, { provider: k8sProvider });

const meiliAppLabels = { app: "meilisearch" };
new k8s.apps.v1.Deployment("meilisearch-deployment", {
	metadata: { name: "meilisearch" },
	spec: {
		selector: { matchLabels: meiliAppLabels },
		replicas: 1,
		// The index lives on a single RWO volume, so never run two pods at once.
		strategy: { type: "Recreate" },
		template: {
			metadata: { labels: meiliAppLabels },
			spec: {
				containers: [
					{
						name: "meilisearch",
						image: "getmeili/meilisearch:v1.11",
						ports: [{ containerPort: 7700 }],
						env: [
							{ name: "MEILI_ENV", value: "production" },
							{
								name: "MEILI_MASTER_KEY",
								valueFrom: {
									secretKeyRef: {
										name: meiliSecret.metadata.name,
										key: "MEILI_MASTER_KEY",
									},
								},
							},
						],
						volumeMounts: [{ name: "data", mountPath: "/meili_data" }],
						readinessProbe: {
							httpGet: { path: "/health", port: 7700 },
							initialDelaySeconds: 5,
							periodSeconds: 10,
						},
						livenessProbe: {
							httpGet: { path: "/health", port: 7700 },
							initialDelaySeconds: 10,
							periodSeconds: 20,
						},
					},
				],
				volumes: [
					{
						name: "data",
						persistentVolumeClaim: { claimName: meiliPvc.metadata.name },
					},
				],
			},
		},
	},
}, { provider: k8sProvider });

const meiliService = new k8s.core.v1.Service("meilisearch-service", {
	metadata: { name: "meilisearch" },
	spec: {
		selector: meiliAppLabels,
		ports: [{ port: 7700, targetPort: 7700 }],
	},
}, { provider: k8sProvider });

// Cluster-internal Meilisearch URL for the service and worker.
const meiliUrl = meiliService.metadata.name.apply(name => `http://${name}:7700`);


const serviceAppLabels = { app: "agon" };
new k8s.apps.v1.Deployment("agon-deployment", {
	metadata: { name: "agon" },
	spec: {
		selector: { matchLabels: serviceAppLabels },
		replicas: 1,
		template: {
			metadata: { labels: serviceAppLabels },
			spec: {
				containers: [
					{
						name: "agon-service",
						image: config.get("agonServiceImage"),
						ports: [{ containerPort: 7000 }],
							envFrom: [{ secretRef: { name: awsSecret.metadata.name } }],
						env: [
							{
								name: "JWT_SECRET",
								valueFrom: {
									secretKeyRef: {
										name: jwtSecret.metadata.name,
										key: 'jwt-secret',
									}
								},
							},
							{
								name: "MEILI_URL",
								value: meiliUrl,
							},
							{
								name: "MEILI_MASTER_KEY",
								valueFrom: {
									secretKeyRef: {
										name: meiliSecret.metadata.name,
										key: "MEILI_MASTER_KEY",
									}
								},
							},
						]
					},
				],
			},
		},
	},
}, { provider: k8sProvider });

// ───────────────────────────────────────────────────────────────────────────
// agon_worker: async processing worker
// Long-polls the SQS events queue (fed by the EventBridge Pipe above) and runs
// the inline handlers — search indexing (Meilisearch) and notification
// generation. Shares the aws-credentials secret (now carrying the queue URL)
// plus the Meilisearch URL/key. No ports / ingress — it only consumes SQS.
// ───────────────────────────────────────────────────────────────────────────
const workerAppLabels = { app: "agon-worker" };
new k8s.apps.v1.Deployment("agon-worker-deployment", {
	metadata: { name: "agon-worker" },
	spec: {
		selector: { matchLabels: workerAppLabels },
		replicas: 1,
		template: {
			metadata: { labels: workerAppLabels },
			spec: {
				containers: [
					{
						name: "agon-worker",
						image: config.get("agonWorkerImage"),
						// AWS creds + table + queue URL come from the shared secret.
						envFrom: [{ secretRef: { name: awsSecret.metadata.name } }],
						env: [
							{
								name: "MEILI_URL",
								value: meiliUrl,
							},
							{
								name: "MEILI_MASTER_KEY",
								valueFrom: {
									secretKeyRef: {
										name: meiliSecret.metadata.name,
										key: "MEILI_MASTER_KEY",
									}
								},
							},
						],
					},
				],
			},
		},
	},
}, { provider: k8sProvider });

const uiAppLabels = { app: "agon-ui" };
new k8s.apps.v1.Deployment("agon-ui-deployment", {
	metadata: { name: "agon-ui" },
	spec: {
		selector: { matchLabels: uiAppLabels },
		replicas: 1,
		template: {
			metadata: { labels: uiAppLabels },
			spec: {
				containers: [
					{
						name: "agon-ui",
						image: config.get("agonUiImage"),
						ports: [{ containerPort: 80 }],
						env: [
							{
								name: "VITE_SUPABASE_URL",
								value: config.get("supabaseUrl"),
							},
							{
								name: "VITE_SUPABASE_ANON_KEY",
								value: config.get("supabaseAnonKey"),
							},
						],
					},
				],
			},
		},
	},
}, { provider: k8sProvider });

const service = new k8s.core.v1.Service("agon-service", {
	metadata: { name: "agon" },
	spec: {
		selector: serviceAppLabels,
		ports: [{ port: 7000, targetPort: 7000 }],
	},
}, { provider: k8sProvider });

const uiService = new k8s.core.v1.Service("agon-ui-service", {
	metadata: { name: "agon-ui" },
	spec: {
		selector: uiAppLabels,
		ports: [{ port: 80, targetPort: 80 }],
	},
}, { provider: k8sProvider });

const fullDomain = `agon.${baseDomain}`;

const certificate = new k8s.apiextensions.CustomResource("agon-cert", {
	apiVersion: "cert-manager.io/v1",
	kind: "Certificate",
	metadata: { namespace: 'default', name: "agon-cert" },
	spec: {
		secretName: "agon-cert",
		issuerRef: { name: issuer.metadata.name, kind: "ClusterIssuer" },
		commonName: fullDomain,
		dnsNames: [fullDomain],
	},
}, { provider: k8sProvider });

new k8s.networking.v1.Ingress("agon-ingress", {
	metadata: {
		namespace: "default",
		annotations: {
			"kubernetes.io/ingress.class": "nginx",
			"cert-manager.io/cluster-issuer": issuer.metadata.name,
		},
	},
	spec: {
		tls: [{
			hosts: [fullDomain],
			secretName: "agon-cert",
		}],
		rules: [{
			host: fullDomain,
			http: {
				paths: [
					{
						path: "/",
						pathType: "Prefix",
						backend: {
							service: {
								name: uiService.metadata.name,
								port: { number: 80 },
							},
						},
					},
				],
			},
		}],
	},
}, { provider: k8sProvider, dependsOn: [ctrl] });

new k8s.networking.v1.Ingress("agon-api-ingress", {
	metadata: {
		namespace: "default",
		annotations: {
			"kubernetes.io/ingress.class": "nginx",
			"cert-manager.io/cluster-issuer": issuer.metadata.name,
			"nginx.ingress.kubernetes.io/rewrite-target": "/$2",
			"nginx.ingress.kubernetes.io/use-regex": "true",
		},
	},
	spec: {
		tls: [{
			hosts: [fullDomain],
			secretName: "agon-cert",
		}],
		rules: [{
			host: fullDomain,
			http: {
				paths: [
					{
						path: "/api(/|$)(.*)",
						pathType: "ImplementationSpecific",
						backend: {
							service: {
								name: service.metadata.name,
								port: { number: 7000 },
							},
						},
					},
				],
			},
		}],
	},
}, { provider: k8sProvider, dependsOn: [ctrl] });

export const temporalDomain = `temporal.${baseDomain}`;
const temporalCertificate = new k8s.apiextensions.CustomResource("temporal-cert", {
	apiVersion: "cert-manager.io/v1",
	kind: "Certificate",
	metadata: { namespace: 'default', name: "temporal-cert" },
	spec: {
		secretName: "temporal-cert",
		issuerRef: { name: issuer.metadata.name, kind: "ClusterIssuer" },
		commonName: temporalDomain,
		dnsNames: [temporalDomain],
	},
}, { provider: k8sProvider });

const temporalIngress = new k8s.networking.v1.Ingress("temporal-ingress", {
	metadata: {
		namespace: "default",
		annotations: {
			"kubernetes.io/ingress.class": "nginx",
			"cert-manager.io/cluster-issuer": issuer.metadata.name,
		},
	},
	spec: {
		tls: [{
			hosts: [temporalDomain],
			secretName: "temporal-cert",
		}],
		rules: [{
			host: temporalDomain,
			http: {
				paths: [{
					path: "/",
					pathType: "Prefix",
					backend: {
						service: {
							name: "temporal-web",
							port: { number: 8080 },
						},
					},
				}],
			},
		}],
	},
}, { provider: k8sProvider, dependsOn: [ctrl] });

export const agonDomain = fullDomain;
