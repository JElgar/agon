import * as pulumi from "@pulumi/pulumi";
import * as command from '@pulumi/command';
import * as k8s from "@pulumi/kubernetes";
import * as cloudflare from "@pulumi/cloudflare";
import * as nginx from "@pulumi/kubernetes-ingress-nginx";
import * as aws from "@pulumi/aws";
import * as gcp from "@pulumi/gcp";
import * as tls from "@pulumi/tls";

const config = new pulumi.Config();
const subdomainPrefix = pulumi.getStack();
const baseDomain = `${subdomainPrefix}.get-agon.com`;

// pulumi config set --secret privateKeyBase64 "$(cat ~/.ssh/pulumi_agon_key | base64)"
const privateKeyBase64 = config.requireSecret("privateKeyBase64");
const privateKey = privateKeyBase64.apply(key => Buffer.from(key, 'base64').toString('utf8'));

// ── OVH VPS (replaces the old Hetzner cpx22) ─────────────────────────────────
// The VPS is provisioned and SSH-keyed OUT OF BAND (via the OVH control panel),
// not by Pulumi: the @ovhcloud/pulumi-ovh provider (v2.15.0) can't read a VPS
// back into state after ordering ("Could not find required property
// 'display_name'"), and its import path is equally broken. Since the VPS is a
// one-time monthly order we never want Pulumi to touch again (a re-order =
// another month's charge), managing it here bought only downside.
//
// MANUAL PREREQUISITES per stack (before `pulumi up`):
//   1. Order the VPS in the OVH panel.
//   2. Reinstall it with Ubuntu 26.04 and inject the public key matching the
//      `privateKeyBase64` stack secret (i.e. ~/.ssh/pulumi_agon_key.pub — paste
//      it in the panel's reinstall dialog), so `ssh root@<ip>` works.
//   3. Pin the node in config:
//        pulumi config set nodeIp <ipv4>
//        pulumi config set nodeServiceName <vpsXXXX.vps.ovh.net>
// staging VPS: vps-2027-model3, 6 vCPU / 12GB / 100GB NVMe, ~£11.13/mo.
// Teardown is a manual cancel in the OVH panel (deliberately not automated).
const nodeIpv4 = config.require("nodeIp");
const nodeServiceName = config.require("nodeServiceName");

const cloudflareZoneId = '3d1b2636aa31acc40c4044830fe4982c';

const dnsRecord = new cloudflare.DnsRecord('record', {
	zoneId: cloudflareZoneId,
	name: `*.${baseDomain}`,
	type: 'A',
	content: nodeIpv4,
	ttl: 300,
	proxied: false,
});

const connection = {
	host: nodeIpv4,
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

const kubeconfig = pulumi.all([getKubeconfig.stdout, nodeIpv4]).apply(([kubeconfig, serverIp]) => {
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

// Register the `default` Temporal namespace. The Helm chart's schema Jobs create
// the DB *schema* but NOT any namespace, so a fresh cluster has none — the UI's
// /namespaces/default/workflows 404s and the worker's task-queue poll has nowhere
// to land. This makes that registration declarative (survives teardown/rebuild)
// instead of a manual `tctl`/`temporal` step.
//
// Runs over the same SSH connection used to install k3s: k3s ships a working
// kubectl + kubeconfig on the node. We `kubectl exec` into the chart's
// admintools pod and create the namespace via its bundled `temporal` CLI.
// Idempotent: `namespace create` errors if it already exists, so we swallow a
// non-zero exit with `|| true` and let the wait-for-rollout be the real gate.
const registerTemporalNamespace = new command.remote.Command("register-temporal-namespace", {
	connection,
	create: [
		// Wait for the frontend to be reachable before registering (the namespace
		// API is served by the frontend, which depends on history/matching).
		"kubectl rollout status deploy/temporal-frontend --timeout=600s",
		// Create `default` with the same 72h retention the rest of the stack uses.
		"kubectl exec deploy/temporal-admintools -- temporal operator namespace create" +
			" --namespace default --address temporal-frontend:7233 --retention 72h || true",
	].join(" && "),
	// Re-run if the connection or command text changes.
	triggers: [nodeIpv4],
}, { dependsOn: [temporal] });

// ───────────────────────────────────────────────────────────────────────────
// AWS: DynamoDB single-table + least-privilege app credentials
// See docs/dynamodb-design.md. All entities live in one table addressed by
// PK/SK, with three overloaded GSIs. Streams are enabled to feed the async
// pipeline (EventBridge Pipe → SQS → worker; see docs/async-design.md).
// ───────────────────────────────────────────────────────────────────────────

// AWS credentials + region are read from the `aws:` config namespace by the
// default provider, exactly like cloudflare:apiToken / ovh:applicationKey. Set them
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
					// Used by the feed fan-out worker (write_feed_items -> BatchWriteItem);
					// without it the FanOutMatch workflow's write_feed_chunk activity
					// fails AccessDenied and retries forever.
					"dynamodb:BatchWriteItem",
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

// CORS: browsers upload the bytes directly to S3 via the presigned PUT, which is
// a cross-origin request from the app. Without this the preflight/PUT is blocked
// with "Access-Control-Allow-Origin missing". Mirrors the service's own CORS
// allowlist (local Vite + the get-agon.com apps). Reads aren't listed because
// objects are served through CloudFront, not the S3 origin, and only GET/PUT are
// used. `ExposeHeaders: ETag` lets the client read the upload response.
new aws.s3.BucketCorsConfigurationV2("agon-assets", {
	bucket: assetsBucket.id,
	corsRules: [
		{
			allowedMethods: ["PUT", "GET", "HEAD"],
			allowedOrigins: ["http://localhost:5173", "https://*.get-agon.com"],
			allowedHeaders: ["*"],
			exposeHeaders: ["ETag"],
			maxAgeSeconds: 3000,
		},
	],
});

// Emit S3 events to EventBridge; the asset pipeline (S3 → EventBridge → SQS →
// worker) uses "object created" to flip a Pending asset to Uploaded and record
// its serving URL. The EventBridge rule + queue are defined further below.
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

// ── CloudFront: private asset delivery ──────────────────────────────────────
// The assets bucket stays fully private (public access blocked above). The ONLY
// reader is this CloudFront distribution, granted via an Origin Access Control
// (OAC) + a bucket policy scoped to this distribution's ARN. Nothing can hit the
// S3 URL directly.
//
// Two behaviours, matching the app's serving model:
//   - profile_image/* and team_image/* → PUBLIC (no trusted key group). Cacheable,
//     shareable, cheap; these images aren't secret.
//   - match_header/*                   → SIGNED (trusted key group). The service
//     mints a short-lived signed URL at read time, so future per-match visibility
//     (e.g. followers-only) is enforced there without changing the upload path.
//
// A custom domain would need an ACM cert in us-east-1; we use the default
// *.cloudfront.net domain for now and expose it to the app as AGON_ASSETS_CDN_URL.

// CloudFront key pair for signed URLs. The PRIVATE half is handed to the service
// (via the k8s secret) to sign match-header URLs; the PUBLIC half is uploaded to
// CloudFront as a public key and referenced by a key group. Generated in-stack so
// the whole pair lives in Pulumi state (encrypted), like the JWT test key.
const assetSigningKey = new tls.PrivateKey("agon-assets-signing", {
	algorithm: "RSA",
	rsaBits: 2048,
});

const assetPublicKey = new aws.cloudfront.PublicKey("agon-assets", {
	name: `agon-assets-${pulumi.getStack()}`,
	comment: "Signs match-header asset URLs",
	encodedKey: assetSigningKey.publicKeyPem,
});

const assetKeyGroup = new aws.cloudfront.KeyGroup("agon-assets", {
	name: `agon-assets-${pulumi.getStack()}`,
	items: [assetPublicKey.id],
});

// OAC: lets CloudFront sign its origin requests to S3 with SigV4, so the bucket
// can trust "requests from this distribution" without being public.
const assetOac = new aws.cloudfront.OriginAccessControl("agon-assets", {
	name: `agon-assets-${pulumi.getStack()}`,
	originAccessControlOriginType: "s3",
	signingBehavior: "always",
	signingProtocol: "sigv4",
});

const assetsCdn = new aws.cloudfront.Distribution("agon-assets", {
	enabled: true,
	comment: `agon assets (${pulumi.getStack()})`,
	// Assets are immutable (content-addressed by asset id), so cache hard.
	defaultRootObject: "",
	origins: [{
		originId: "assets-s3",
		domainName: assetsBucket.bucketRegionalDomainName,
		originAccessControlId: assetOac.id,
	}],
	// Default behaviour = public (covers profile_image/* and team_image/*).
	defaultCacheBehavior: {
		targetOriginId: "assets-s3",
		viewerProtocolPolicy: "redirect-to-https",
		allowedMethods: ["GET", "HEAD"],
		cachedMethods: ["GET", "HEAD"],
		compress: true,
		forwardedValues: {
			queryString: false,
			cookies: { forward: "none" },
		},
		minTtl: 0,
		defaultTtl: 86400,
		maxTtl: 31536000,
	},
	// match_header/* = signed: only requests bearing a valid signature from the
	// key group are served.
	orderedCacheBehaviors: [{
		pathPattern: "match_header/*",
		targetOriginId: "assets-s3",
		viewerProtocolPolicy: "redirect-to-https",
		allowedMethods: ["GET", "HEAD"],
		cachedMethods: ["GET", "HEAD"],
		compress: true,
		trustedKeyGroups: [assetKeyGroup.id],
		forwardedValues: {
			queryString: false,
			cookies: { forward: "none" },
		},
		minTtl: 0,
		defaultTtl: 3600,
		maxTtl: 86400,
	}],
	restrictions: {
		geoRestriction: { restrictionType: "none" },
	},
	viewerCertificate: {
		cloudfrontDefaultCertificate: true,
	},
	priceClass: "PriceClass_100",
});

// Bucket policy: allow ONLY this distribution to read objects (OAC principal +
// SourceArn condition). This is what keeps the bucket private while CloudFront
// serves it.
new aws.s3.BucketPolicy("agon-assets-cloudfront", {
	bucket: assetsBucket.id,
	policy: pulumi.jsonStringify({
		Version: "2012-10-17",
		Statement: [{
			Sid: "AllowCloudFrontRead",
			Effect: "Allow",
			Principal: { Service: "cloudfront.amazonaws.com" },
			Action: "s3:GetObject",
			Resource: pulumi.interpolate`${assetsBucket.arn}/*`,
			Condition: {
				StringEquals: { "AWS:SourceArn": assetsCdn.arn },
			},
		}],
	}),
});

// The base serving URL handed to the service/worker. The service builds public
// URLs and signs match-header URLs against this; the worker stores the canonical
// URL on an asset when it uploads.
const assetsCdnUrl = pulumi.interpolate`https://${assetsCdn.domainName}`;

// ── Asset events: S3 → EventBridge → SQS → agon_worker ──────────────────────
// On "Object Created" in the assets bucket, EventBridge routes the event to a
// dedicated SQS queue the worker long-polls; the worker flips the asset to
// Uploaded. Separate from the DynamoDB-stream events queue (different message
// shape and failure domain).
const assetEventsDlq = new aws.sqs.Queue("agon-asset-events-dlq", {
	name: `agon-asset-events-dlq-${pulumi.getStack()}`,
	messageRetentionSeconds: 1209600, // 14 days.
});

const assetEventsQueue = new aws.sqs.Queue("agon-asset-events", {
	name: `agon-asset-events-${pulumi.getStack()}`,
	visibilityTimeoutSeconds: 60,
	messageRetentionSeconds: 345600, // 4 days.
	redrivePolicy: pulumi.jsonStringify({
		deadLetterTargetArn: assetEventsDlq.arn,
		maxReceiveCount: 5,
	}),
});

// EventBridge rule matching S3 object-created events for this bucket only.
const assetEventsRule = new aws.cloudwatch.EventRule("agon-asset-events", {
	name: `agon-asset-events-${pulumi.getStack()}`,
	description: "S3 object-created in the assets bucket → asset events queue",
	eventPattern: assetsBucket.bucket.apply(name => JSON.stringify({
		source: ["aws.s3"],
		"detail-type": ["Object Created"],
		detail: { bucket: { name: [name] } },
	})),
});

new aws.cloudwatch.EventTarget("agon-asset-events", {
	rule: assetEventsRule.name,
	arn: assetEventsQueue.arn,
});

// Allow EventBridge to send matched events to the queue.
new aws.sqs.QueuePolicy("agon-asset-events", {
	queueUrl: assetEventsQueue.id,
	policy: pulumi.jsonStringify({
		Version: "2012-10-17",
		Statement: [{
			Sid: "AllowEventBridge",
			Effect: "Allow",
			Principal: { Service: "events.amazonaws.com" },
			Action: "sqs:SendMessage",
			Resource: assetEventsQueue.arn,
			Condition: { ArnEquals: { "aws:SourceArn": assetEventsRule.arn } },
		}],
	}),
});

// The worker consumes this queue too (in addition to the main events queue).
new aws.iam.UserPolicy("agon-app-asset-events-sqs", {
	user: appAwsUser.name,
	policy: pulumi.jsonStringify({
		Version: "2012-10-17",
		Statement: [{
			Sid: "ConsumeAssetEventsQueue",
			Effect: "Allow",
			Action: [
				"sqs:ReceiveMessage",
				"sqs:DeleteMessage",
				"sqs:GetQueueAttributes",
			],
			Resource: [assetEventsQueue.arn],
		}],
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
		AGON_ASSETS_CDN_URL: assetsCdnUrl,
		AGON_EVENTS_QUEUE_URL: eventsQueue.url,
		AGON_ASSET_EVENTS_QUEUE_URL: assetEventsQueue.url,
		// CloudFront signed-URL key pair for match-header serving. The service
		// signs with the private half; the id names the public key CloudFront trusts.
		AGON_CLOUDFRONT_KEY_PAIR_ID: assetPublicKey.id,
		AGON_CLOUDFRONT_PRIVATE_KEY: assetSigningKey.privateKeyPem,
	},
}, { provider: k8sProvider });

// ── JWT auth (asymmetric) ────────────────────────────────────────────────────
// The service verifies tokens against public keys only — no shared secret:
//   - `supabaseJwksUrl`: Supabase's JWKS endpoint (real user tokens).
//   - `agonStaticJwks`:  a static JWK set trusting the integration-test signing
//                        key (public half). Public → plain config, not secret.
// The matching PRIVATE test key is the one real secret; it's stored encrypted in
// Pulumi (`agonTestJwtPrivateKey`) so the whole keypair lives in one place, and
// exported below for the CI test job to sign with. Rotate both together.
//   pulumi config set supabaseJwksUrl https://<project>.supabase.co/auth/v1/.well-known/jwks.json
//   pulumi config set agonStaticJwks '{"keys":[...]}'
//   pulumi config set --secret agonTestJwtPrivateKey "$(cat test_ec_pkcs8.pem)"
export const agonTestJwtPrivateKey = config.requireSecret("agonTestJwtPrivateKey");

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
							// Expose the Prometheus /metrics endpoint (experimental).
							// Scraped by the ServiceMonitor below. Requires a bearer key
							// with `metrics.get`, so Prometheus authenticates with the
							// master key (see the ServiceMonitor's bearerTokenSecret).
							{ name: "MEILI_EXPERIMENTAL_ENABLE_METRICS", value: "true" },
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
		// Port is named so the Meilisearch ServiceMonitor can reference it.
		ports: [{ name: "http", port: 7700, targetPort: 7700 }],
	},
}, { provider: k8sProvider });

// Cluster-internal Meilisearch URL for the service and worker.
const meiliUrl = meiliService.metadata.name.apply(name => `http://${name}:7700`);


// ───────────────────────────────────────────────────────────────────────────
// GCP: Firebase Cloud Messaging (push notifications)
//
// Pulumi fully manages the GCP project here (unlike the OVH VPS above, which
// is manual because of a genuine `pulumi-ovh` provider bug — that limitation
// doesn't apply to the GCP provider). Creating the project needs:
//   pulumi config set gcp:project <new-project-id>
//   pulumi config set gcp:region europe-west2
// `gcp:orgId` and `gcp:billingAccount` are both OPTIONAL:
//   pulumi config set gcp:orgId <org-id>            # only if under a formal GCP Organization
//   pulumi config set gcp:billingAccount <billing-account-id>  # only if something here needs the Blaze plan
// Leave `orgId` unset for a personal-account project (Console shows "No
// Organization" / org id `0`) — same as the existing "agon" project. Leave
// `billingAccount` unset too: FCM (and Firebase's Spark/no-cost plan
// generally) doesn't require billing, and setting up a billing account needs
// a real payment method — not worth forcing on someone just for push
// notifications. Both create the project the same way, just narrower.
// Also needs ambient GCP credentials with `roles/resourcemanager.projectCreator`
// (at the org level if `orgId` is set, otherwise on the calling account).
// `deletionPolicy: "PREVENT"` blocks the one genuinely dangerous accident
// this unlocks — a `pulumi destroy` (or a projectId change forcing
// replacement) taking the whole project, and everything in it, with it.
//
// Each full stage (staging, prod) gets its own, fully separate GCP project —
// same principle as the per-stack DynamoDB table / S3 bucket above, so a
// staging change can be tested before it ever touches prod, and a new stage
// needs zero manual GCP setup. This is deliberately NOT shared across stages,
// and NOT meant to be created per lightweight/preview stack (a future PR-
// preview stack kind should attach to an existing stage's project instead of
// minting a new one — out of scope here).
//
// The project is also where Supabase's Google-sign-in OAuth client lives —
// see the "Supabase Google Auth" block below, which is entirely manual
// (Google shut down the only API for automating it in July 2025).
//
// The worker (agon_worker) is the only FCM consumer: device registration just
// writes to DynamoDB (see agon_service's /devices endpoints), and only the
// worker's push handler calls FCM to actually send.
// ───────────────────────────────────────────────────────────────────────────
const gcpConfig = new pulumi.Config("gcp");
const gcpProjectId = gcpConfig.require("project");
// Optional: omitted entirely (not just passed as undefined-but-present) for a
// personal-account project with no GCP Organization.
const gcpOrgId = gcpConfig.get("orgId");
// Optional too: FCM (and Firebase's Spark/no-cost plan generally) doesn't
// need a billing account, and creating one requires a real payment method —
// not something to force on someone just to ship push notifications. Set
// gcp:billingAccount later if some other API this project ends up using
// requires the Blaze plan.
const gcpBillingAccount = gcpConfig.get("billingAccount");

const gcpProject = new gcp.organizations.Project("agon-firebase-project", {
	projectId: gcpProjectId,
	name: `agon-${pulumi.getStack()}`,
	...(gcpOrgId ? { orgId: gcpOrgId } : {}),
	...(gcpBillingAccount ? { billingAccount: gcpBillingAccount } : {}),
	deletionPolicy: "PREVENT",
});

const fcmApi = new gcp.projects.Service("fcm-api", {
	project: gcpProject.projectId,
	service: "fcm.googleapis.com",
	// Never let a `destroy` disable the API on the project — only Pulumi's own
	// resources here should be torn down (the project itself is guarded above).
	disableOnDestroy: false,
});

const firebaseApi = new gcp.projects.Service("firebase-api", {
	project: gcpProject.projectId,
	service: "firebase.googleapis.com",
	disableOnDestroy: false,
});

// Attaches Firebase to the project just created.
const firebaseProject = new gcp.firebase.Project("agon-firebase", {
	project: gcpProject.projectId,
}, { dependsOn: [firebaseApi] });

// Registers the PWA as a Firebase Web App, which is what the client SDK needs
// to request an FCM token. Future: gcp.firebase.AndroidApp / AppleApp once
// those clients exist — same project, no changes needed here.
new gcp.firebase.WebApp("agon-pwa", {
	project: gcpProject.projectId,
	displayName: `agon-${pulumi.getStack()}`,
}, { dependsOn: [firebaseProject] });

// ── Supabase Google Auth: OAuth consent screen + client ─────────────────────
// Fully manual, per project — and NOT automatable at all right now, not even
// partially. This used to create the OAuth consent screen ("Brand") via
// `gcp.iap.Brand`, but that resource depends on the IAP OAuth Admin API,
// which Google shut down entirely in July 2025 (see
// https://docs.cloud.google.com/iap/docs/deprecations/migrate-oauth-client) —
// `gcp.iap.Brand`/`gcp.iap.Client` no longer function for anyone, regardless
// of billing/org status. There is currently no replacement API for
// programmatic OAuth consent screen or client creation outside the Console.
// One-time steps per project:
//   1. Console → APIs & Services → OAuth consent screen → configure the
//      support email + app name (this is the "Brand" — one per project,
//      permanent, can't be deleted via API or Console once created).
//   2. Console → APIs & Services → Credentials → Create Credentials →
//      OAuth client ID → Web application.
//   3. Authorized redirect URI: https://<supabase-project-ref>.supabase.co/auth/v1/callback
//   4. Paste the resulting Client ID + Secret into the Supabase dashboard
//      (Authentication → Providers → Google).

// The service account the worker authenticates as (FCM HTTP v1's OAuth2
// service-account flow). Scoped to exactly one role — sending messages — not
// general Firebase admin.
const fcmServiceAccount = new gcp.serviceaccount.Account("agon-fcm-sender", {
	project: gcpProject.projectId,
	accountId: `agon-fcm-${pulumi.getStack()}`,
	displayName: `agon FCM sender (${pulumi.getStack()})`,
}, { dependsOn: [firebaseApi] });

new gcp.projects.IAMMember("agon-fcm-sender-role", {
	project: gcpProject.projectId,
	role: "roles/firebasecloudmessaging.admin",
	member: pulumi.interpolate`serviceAccount:${fcmServiceAccount.email}`,
});

// The actual credential the worker signs its OAuth2 JWT bearer assertions
// with. Pulumi state holds it encrypted, same as the CloudFront signing key
// and the test JWT private key above.
const fcmServiceAccountKey = new gcp.serviceaccount.Key("agon-fcm-sender-key", {
	serviceAccountId: fcmServiceAccount.name,
});

// A standalone k8s Secret (mirrors `meiliSecret`, not folded into
// `awsSecret`) so the FCM credential is separately rotatable. GCP returns the
// key already base64-encoded; decode it so the env var is the raw JSON the
// worker's `serde_json::from_str` expects.
const fcmSecret = new k8s.core.v1.Secret("fcm-credentials", {
	metadata: { name: "fcm-credentials", namespace: "default" },
	type: "Opaque",
	stringData: {
		AGON_FCM_SERVICE_ACCOUNT_JSON: fcmServiceAccountKey.privateKey.apply(
			key => Buffer.from(key, "base64").toString("utf8")),
	},
}, { provider: k8sProvider });


// ───────────────────────────────────────────────────────────────────────────
// Observability: LGTM stack (Loki, Grafana, Tempo, Mimir-less) + kube-prometheus
//
// Grafana is the single UI. Behind it:
//   * kube-prometheus-stack — Prometheus Operator (satisfies the CNPG
//     `enablePodMonitor` above), Prometheus, node-exporter, kube-state-metrics,
//     AND Grafana with datasources pre-provisioned.
//   * Loki  (single-binary, filesystem) — logs.
//   * Tempo (single-binary, filesystem) — traces.
//   * Alloy — one OTLP collector the apps push to; it fans out
//       traces  → Tempo, logs → Loki, metrics → Prometheus (remote-write).
//
// The apps push over OTLP/gRPC to `alloy.observability:4317`; only Alloy knows
// the individual backends, so swapping/scaling them never touches the Rust.
//
// ⚠️ Sizing: this whole stack runs on the single OVH VPS-3 node (12 GB). Every
// component below sets tight resource requests/limits and short retention. If
// the node starts OOMing, the first levers are bumping the node type or cutting
// retention further. See docs/observability.md.
// ───────────────────────────────────────────────────────────────────────────
const observabilityNs = new k8s.core.v1.Namespace("observability", {
	metadata: { name: "observability" },
}, { provider: k8sProvider });

const observabilityNamespace = observabilityNs.metadata.name;

// Grafana admin password, held as a stack secret:
//   pulumi config set --secret grafanaAdminPassword <password>
const grafanaAdminPassword = config.requireSecret("grafanaAdminPassword");

// kube-prometheus-stack: Prometheus Operator + Prometheus + Grafana + cluster
// metrics. Grafana datasources for Prometheus/Loki/Tempo are provisioned inline
// so the UI works out of the box, with trace↔log correlation wired between
// Tempo and Loki. Prometheus is given a remote-write receiver so Alloy can push
// the apps' OTLP metrics into it.
const kubePrometheusStack = new k8s.helm.v4.Chart("kube-prometheus-stack", {
	chart: "kube-prometheus-stack",
	version: "77.6.2",
	repositoryOpts: { repo: "https://prometheus-community.github.io/helm-charts" },
	namespace: observabilityNamespace,
	values: {
		// Watch ServiceMonitors/PodMonitors across all namespaces (CNPG's live
		// in the default namespace), not just those with the release's labels.
		prometheus: {
			prometheusSpec: {
				serviceMonitorSelectorNilUsesHelmValues: false,
				podMonitorSelectorNilUsesHelmValues: false,
				ruleSelectorNilUsesHelmValues: false,
				// Accept remote-write + OTLP from Alloy.
				enableRemoteWriteReceiver: true,
				enableFeatures: ["otlp-write-receiver"],
				retention: "7d",
				retentionSize: "6GB",
				resources: {
					requests: { cpu: "100m", memory: "400Mi" },
					limits: { memory: "900Mi" },
				},
				storageSpec: {
					volumeClaimTemplate: {
						spec: {
							accessModes: ["ReadWriteOnce"],
							resources: { requests: { storage: "8Gi" } },
						},
					},
				},
			},
		},
		// Trim the pieces that don't earn their memory on a single-node cluster.
		alertmanager: { enabled: false },
		// k3s runs these as static pods the operator can't scrape by default;
		// disabling the ServiceMonitors avoids noisy "down" targets. Re-enable
		// per-component with the right endpoints if you want control-plane metrics.
		kubeControllerManager: { enabled: false },
		kubeScheduler: { enabled: false },
		kubeProxy: { enabled: false },
		kubeEtcd: { enabled: false },
		prometheusOperator: {
			// The admission webhook (validates PrometheusRule/AlertmanagerConfig CRDs)
			// normally gets its TLS secret from the chart's Helm HOOK Jobs
			// (pre-install/post-install `admission-create`/`-patch`). Pulumi's
			// helm.v4.Chart does NOT run Helm hooks, so those Jobs never run and the
			// operator would hang forever mounting the missing
			// `kube-prometheus-stack-admission` secret. Instead of disabling the
			// webhook, we let cert-manager issue the cert — it creates a self-signed
			// Issuer + Certificate (CRs, not hooks), so there's no hook dependency and
			// CRD validation stays on. `patch` is the hook-Job path; keep it disabled.
			admissionWebhooks: {
				enabled: true,
				patch: { enabled: false },
				certManager: { enabled: true },
			},
			resources: {
				requests: { cpu: "50m", memory: "100Mi" },
				limits: { memory: "250Mi" },
			},
		},
		"kube-state-metrics": {
			resources: {
				requests: { cpu: "20m", memory: "48Mi" },
				limits: { memory: "128Mi" },
			},
		},
		"prometheus-node-exporter": {
			resources: {
				requests: { cpu: "20m", memory: "32Mi" },
				limits: { memory: "64Mi" },
			},
		},
		grafana: {
			adminPassword: grafanaAdminPassword,
			resources: {
				requests: { cpu: "50m", memory: "128Mi" },
				limits: { memory: "300Mi" },
			},
			// Persist dashboards/settings across restarts.
			persistence: { enabled: true, size: "1Gi" },
			// Datasources for the other two signals. Prometheus is added by the
			// chart itself; we add Loki + Tempo and wire correlation.
			additionalDataSources: [
				{
					name: "Loki",
					type: "loki",
					access: "proxy",
					url: "http://loki.observability:3100",
					jsonData: {
						// Turn a log line's trace id into a Tempo link.
						derivedFields: [
							{
								name: "trace_id",
								matcherRegex: "trace_id=(\\w+)",
								datasourceUid: "tempo",
								url: "$${__value.raw}",
							},
						],
					},
				},
				{
					name: "Tempo",
					uid: "tempo",
					type: "tempo",
					access: "proxy",
					url: "http://tempo.observability:3100",
					jsonData: {
						// Jump from a span to its logs in Loki.
						tracesToLogsV2: {
							datasourceUid: "loki",
							filterByTraceID: true,
						},
					},
				},
			],
		},
	},
	// The operator's admission webhook now gets its TLS cert from cert-manager
	// (see admissionWebhooks.certManager above), so cert-manager's CRDs + webhook
	// must be ready before this chart applies.
}, {
	provider: k8sProvider,
	dependsOn: [certManager],
	// Every `pulumi up` re-renders this chart, and two of its resources never
	// converge — each diff would otherwise churn on every deploy:
	//   * The Grafana PVC: the chart template omits `spec.volumeName`, but once
	//     Kubernetes binds the PVC it stamps the bound PV name into that IMMUTABLE
	//     field. Pulumi then sees desired(no volumeName) != live(bound) and, unable
	//     to patch an immutable field, REPLACES the PVC — deleting the underlying
	//     volume (reclaim policy Delete) and taking Grafana down with a 502 until it
	//     reprovisions. Ignoring `spec.volumeName` keeps the bound PVC in place.
	//   * The Grafana Role: the chart renders `rules: []`; the apiserver drops the
	//     empty field, so Pulumi perpetually diffs `+ rules: []`. Harmless, but noise.
	// Scope the ignore to just those two resources so real chart changes still apply.
	transforms: [(args) => {
		const ignore =
			args.type === "kubernetes:core/v1:PersistentVolumeClaim" &&
			args.name.includes("grafana")
				? ["spec.volumeName"]
				: args.type === "kubernetes:rbac.authorization.k8s.io/v1:Role" &&
						args.name.includes("grafana")
					? ["rules"]
					: undefined;
		return ignore
			? { props: args.props, opts: pulumi.mergeOptions(args.opts, { ignoreChanges: ignore }) }
			: undefined;
	}],
});

// Loki — single-binary, filesystem-backed. Enough for one node; not HA. The
// `test`/`lokiCanary` helpers and gateway are off to save resources.
const loki = new k8s.helm.v4.Chart("loki", {
	chart: "loki",
	version: "6.24.0",
	repositoryOpts: { repo: "https://grafana.github.io/helm-charts" },
	namespace: observabilityNamespace,
	values: {
		deploymentMode: "SingleBinary",
		loki: {
			auth_enabled: false,
			commonConfig: { replication_factor: 1 },
			storage: { type: "filesystem" },
			schemaConfig: {
				configs: [
					{
						from: "2024-01-01",
						store: "tsdb",
						object_store: "filesystem",
						schema: "v13",
						index: { prefix: "index_", period: "24h" },
					},
				],
			},
			// Drop logs older than ~3 days to bound disk on the small node.
			limits_config: { retention_period: "72h" },
		},
		singleBinary: {
			replicas: 1,
			persistence: { enabled: true, size: "5Gi" },
			resources: {
				requests: { cpu: "100m", memory: "128Mi" },
				limits: { memory: "400Mi" },
			},
		},
		// Single-binary mode: turn off all the distributed component deployments.
		backend: { replicas: 0 },
		read: { replicas: 0 },
		write: { replicas: 0 },
		gateway: { enabled: false },
		chunksCache: { enabled: false },
		resultsCache: { enabled: false },
		lokiCanary: { enabled: false },
		test: { enabled: false },
		monitoring: { selfMonitoring: { enabled: false }, lokiCanary: { enabled: false } },
	},
}, { provider: k8sProvider });

// Tempo — single-binary traces backend with the OTLP receiver enabled. Alloy
// forwards spans here; Grafana queries it on port 3100.
const tempo = new k8s.helm.v4.Chart("tempo", {
	chart: "tempo",
	version: "1.18.2",
	repositoryOpts: { repo: "https://grafana.github.io/helm-charts" },
	namespace: observabilityNamespace,
	values: {
		tempo: {
			retention: "72h",
			storage: {
				trace: { backend: "local", local: { path: "/var/tempo/traces" } },
			},
			resources: {
				requests: { cpu: "100m", memory: "128Mi" },
				limits: { memory: "400Mi" },
			},
		},
		persistence: { enabled: true, size: "5Gi" },
	},
}, { provider: k8sProvider });

// Grafana Alloy — the single OTLP collector the apps push to. Receives OTLP on
// 4317 (gRPC) / 4318 (HTTP) and fans out: traces→Tempo, logs→Loki,
// metrics→Prometheus remote-write. Config is Alloy's River syntax.
const alloyConfig = `
otelcol.receiver.otlp "default" {
  grpc { endpoint = "0.0.0.0:4317" }
  http { endpoint = "0.0.0.0:4318" }

  output {
    metrics = [otelcol.processor.batch.default.input]
    logs    = [otelcol.processor.batch.default.input]
    traces  = [otelcol.processor.batch.default.input]
  }
}

otelcol.processor.batch "default" {
  output {
    metrics = [otelcol.exporter.prometheus.default.input]
    logs    = [otelcol.exporter.loki.default.input]
    traces  = [otelcol.exporter.otlp.tempo.input]
  }
}

// Traces → Tempo (OTLP gRPC on 4317, insecure in-cluster).
otelcol.exporter.otlp "tempo" {
  client {
    endpoint = "tempo.observability:4317"
    tls { insecure = true }
  }
}

// Logs → Loki (OTLP HTTP push endpoint).
otelcol.exporter.loki "default" {
  forward_to = [loki.write.default.receiver]
}
loki.write "default" {
  endpoint { url = "http://loki.observability:3100/loki/api/v1/push" }
}

// Metrics → Prometheus remote-write.
otelcol.exporter.prometheus "default" {
  forward_to = [prometheus.remote_write.default.receiver]
}
prometheus.remote_write "default" {
  endpoint { url = "http://kube-prometheus-stack-prometheus.observability:9090/api/v1/write" }
}
`;

const alloyConfigMap = new k8s.core.v1.ConfigMap("alloy-config", {
	metadata: { name: "alloy-config", namespace: observabilityNamespace },
	data: { "config.alloy": alloyConfig },
}, { provider: k8sProvider });

const alloy = new k8s.helm.v4.Chart("alloy", {
	chart: "alloy",
	version: "1.2.1",
	repositoryOpts: { repo: "https://grafana.github.io/helm-charts" },
	namespace: observabilityNamespace,
	values: {
		alloy: {
			configMap: { create: false, name: alloyConfigMap.metadata.name },
			// Expose the OTLP ports on the Alloy Service so the apps can reach it.
			extraPorts: [
				{ name: "otlp-grpc", port: 4317, targetPort: 4317, protocol: "TCP" },
				{ name: "otlp-http", port: 4318, targetPort: 4318, protocol: "TCP" },
			],
			resources: {
				requests: { cpu: "50m", memory: "128Mi" },
				limits: { memory: "300Mi" },
			},
		},
		// Single collector instance; no clustering / node-local agent needed.
		controller: { type: "deployment", replicas: 1 },
	},
}, { provider: k8sProvider, dependsOn: [loki, tempo, kubePrometheusStack] });

// The OTLP endpoint both apps push to. In-cluster gRPC on the Alloy Service.
const otlpEndpoint = "http://alloy.observability:4317";

// ── Grafana ingress ──────────────────────────────────────────────────────────
// Public TLS ingress at grafana.<stack>.get-agon.com, mirroring the agon /
// temporal ingresses. Grafana's Service is created by kube-prometheus-stack.
const grafanaDomain = `grafana.${baseDomain}`;
const grafanaServiceName = "kube-prometheus-stack-grafana";

const grafanaCertificate = new k8s.apiextensions.CustomResource("grafana-cert", {
	apiVersion: "cert-manager.io/v1",
	kind: "Certificate",
	metadata: { namespace: "default", name: "grafana-cert" },
	spec: {
		secretName: "grafana-cert",
		issuerRef: { name: issuer.metadata.name, kind: "ClusterIssuer" },
		commonName: grafanaDomain,
		dnsNames: [grafanaDomain],
	},
}, { provider: k8sProvider });

new k8s.networking.v1.Ingress("grafana-ingress", {
	metadata: {
		namespace: "default",
		annotations: {
			"kubernetes.io/ingress.class": "nginx",
			"cert-manager.io/cluster-issuer": issuer.metadata.name,
		},
	},
	spec: {
		tls: [{ hosts: [grafanaDomain], secretName: "grafana-cert" }],
		rules: [{
			host: grafanaDomain,
			http: {
				paths: [{
					path: "/",
					pathType: "Prefix",
					backend: {
						// Grafana runs in the observability namespace; this ingress is
						// in default. An ExternalName service bridges the namespaces.
						service: { name: "grafana-proxy", port: { number: 80 } },
					},
				}],
			},
		}],
	},
}, { provider: k8sProvider, dependsOn: [ctrl, kubePrometheusStack] });

// Cross-namespace bridge: an ExternalName Service in `default` pointing at the
// Grafana Service in `observability`, so the ingress backend (which must live
// in the same namespace as the Ingress) can reach it.
new k8s.core.v1.Service("grafana-proxy", {
	metadata: { name: "grafana-proxy", namespace: "default" },
	spec: {
		type: "ExternalName",
		externalName: `${grafanaServiceName}.observability.svc.cluster.local`,
		// The Grafana Service in observability listens on port 80 (it maps 80→3000
		// internally). nginx-ingress dials the resolved ClusterIP on this port, so
		// it must be 80, not Grafana's container port 3000 — otherwise nginx hits
		// ClusterIP:3000, where nothing listens, and every request 504s.
		ports: [{ port: 80, targetPort: 80 }],
	},
}, { provider: k8sProvider });

export const grafanaUrl = `https://${grafanaDomain}`;

// ── Meilisearch scraping ──────────────────────────────────────────────────────
// Meilisearch's /metrics endpoint is experimental (enabled via
// MEILI_EXPERIMENTAL_ENABLE_METRICS on the deployment) and requires a bearer key
// with the `metrics.get` action — we use the master key. This ServiceMonitor
// lives in `default` (same namespace as the meilisearch Service + its master-key
// secret) and is discovered by the Prometheus Operator, which is configured
// (serviceMonitorSelectorNilUsesHelmValues: false) to watch all namespaces.
new k8s.apiextensions.CustomResource("meilisearch-servicemonitor", {
	apiVersion: "monitoring.coreos.com/v1",
	kind: "ServiceMonitor",
	metadata: { name: "meilisearch", namespace: "default" },
	spec: {
		selector: { matchLabels: meiliAppLabels },
		endpoints: [{
			// References the meilisearch Service's port by name (named "http" below).
			port: "http",
			path: "/metrics",
			interval: "30s",
			// Authenticate with the master key from the existing secret.
			bearerTokenSecret: { name: meiliSecret.metadata.name, key: "MEILI_MASTER_KEY" },
		}],
	},
}, { provider: k8sProvider, dependsOn: [kubePrometheusStack] });

// ── Grafana dashboards ────────────────────────────────────────────────────────
// The kube-prometheus-stack Grafana runs a sidecar that auto-imports any
// ConfigMap in its namespace labelled `grafana_dashboard: "1"`. We ship two:
// the agon API (our custom request metrics) and Meilisearch. Metric names assume
// Alloy's prometheus exporter suffix conversion (dots→underscores, `_total` on
// counters, unit `_seconds` on the duration histogram).

// agon-service: request rate, error rate, and p50/p95 latency from the
// http.server.request.* instruments recorded in the request middleware.
const agonDashboard = {
	title: "Agon — API",
	uid: "agon-api",
	timezone: "browser",
	time: { from: "now-6h", to: "now" },
	panels: [
		{
			id: 1, title: "Request rate (req/s)", type: "timeseries",
			gridPos: { h: 8, w: 12, x: 0, y: 0 },
			targets: [{
				expr: "sum by (http_response_status_code) (rate(http_server_request_count_total{job=\"agon-service\"}[5m]))",
				legendFormat: "{{http_response_status_code}}",
			}],
		},
		{
			id: 2, title: "5xx error rate", type: "timeseries",
			gridPos: { h: 8, w: 12, x: 12, y: 0 },
			targets: [{
				expr: "sum(rate(http_server_request_count_total{http_response_status_code=~\"5..\"}[5m]))",
				legendFormat: "5xx",
			}],
		},
		{
			id: 3, title: "Latency p50 / p95 (s)", type: "timeseries",
			gridPos: { h: 8, w: 24, x: 0, y: 8 },
			targets: [
				{
					expr: "histogram_quantile(0.50, sum by (le) (rate(http_server_request_duration_seconds_bucket[5m])))",
					legendFormat: "p50",
				},
				{
					expr: "histogram_quantile(0.95, sum by (le) (rate(http_server_request_duration_seconds_bucket[5m])))",
					legendFormat: "p95",
				},
			],
		},
	],
	schemaVersion: 39,
};

// Meilisearch: index docs, DB size, HTTP request rate, task queue latency.
const meiliDashboard = {
	title: "Meilisearch",
	uid: "meilisearch",
	timezone: "browser",
	time: { from: "now-6h", to: "now" },
	panels: [
		{
			id: 1, title: "Documents per index", type: "timeseries",
			gridPos: { h: 8, w: 12, x: 0, y: 0 },
			targets: [{ expr: "meilisearch_index_docs_count", legendFormat: "{{index}}" }],
		},
		{
			id: 2, title: "DB size (bytes)", type: "timeseries",
			gridPos: { h: 8, w: 12, x: 12, y: 0 },
			targets: [
				{ expr: "meilisearch_db_size_bytes", legendFormat: "total" },
				{ expr: "meilisearch_used_db_size_bytes", legendFormat: "used" },
			],
		},
		{
			id: 3, title: "HTTP requests (req/s)", type: "timeseries",
			gridPos: { h: 8, w: 12, x: 0, y: 8 },
			targets: [{
				expr: "sum by (path) (rate(meilisearch_http_requests_total[5m]))",
				legendFormat: "{{path}}",
			}],
		},
		{
			id: 4, title: "Task queue latency (s)", type: "timeseries",
			gridPos: { h: 8, w: 12, x: 12, y: 8 },
			targets: [{ expr: "meilisearch_task_queue_latency_seconds", legendFormat: "latency" }],
		},
	],
	schemaVersion: 39,
};

new k8s.core.v1.ConfigMap("agon-dashboard", {
	metadata: {
		name: "agon-dashboard",
		namespace: observabilityNamespace,
		labels: { grafana_dashboard: "1" },
	},
	data: { "agon-api.json": JSON.stringify(agonDashboard) },
}, { provider: k8sProvider, dependsOn: [kubePrometheusStack] });

new k8s.core.v1.ConfigMap("meilisearch-dashboard", {
	metadata: {
		name: "meilisearch-dashboard",
		namespace: observabilityNamespace,
		labels: { grafana_dashboard: "1" },
	},
	data: { "meilisearch.json": JSON.stringify(meiliDashboard) },
}, { provider: k8sProvider, dependsOn: [kubePrometheusStack] });

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
							// JWT auth is asymmetric: the service verifies tokens against
							// Supabase's JWKS (real users) and a static JWK set (the test
							// signing key). No shared secret.
							{
								name: "SUPABASE_JWKS_URL",
								value: config.get("supabaseJwksUrl"),
							},
							{
								name: "AGON_STATIC_JWKS",
								value: config.get("agonStaticJwks"),
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
							// OTLP export to the Alloy collector. Unset ⇒ stdout logs only.
							{ name: "OTEL_EXPORTER_OTLP_ENDPOINT", value: otlpEndpoint },
							{ name: "OTEL_SERVICE_NAME", value: "agon-service" },
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
// the inline handlers — search indexing (Meilisearch), notification
// generation, and FCM push delivery. Shares the aws-credentials secret (now
// carrying the queue URL) plus the Meilisearch URL/key and the fcm-credentials
// secret above. No ports / ingress — it only consumes SQS.
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
							// FCM sender credential — only the worker calls FCM (device
							// registration itself is a plain DynamoDB write from agon_service).
							{
								name: "AGON_FCM_SERVICE_ACCOUNT_JSON",
								valueFrom: {
									secretKeyRef: {
										name: fcmSecret.metadata.name,
										key: "AGON_FCM_SERVICE_ACCOUNT_JSON",
									}
								},
							},
							// Temporal connection. The SDK config loader reads these
							// (TEMPORAL_ADDRESS / TEMPORAL_NAMESPACE); it otherwise defaults to
							// http://localhost:7233, which would crash-loop the worker since a
							// failed connect is fatal (see agon_worker/src/main.rs).
							// `temporal-frontend` is the Helm chart gRPC frontend Service (7233);
							// `temporal-web` (used by the ingress) is the UI only.
							{ name: "TEMPORAL_ADDRESS", value: "temporal-frontend:7233" },
							{ name: "TEMPORAL_NAMESPACE", value: "default" },
							// OTLP export to the Alloy collector. Unset ⇒ stdout logs only.
							{ name: "OTEL_EXPORTER_OTLP_ENDPOINT", value: otlpEndpoint },
							{ name: "OTEL_SERVICE_NAME", value: "agon-worker" },
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
