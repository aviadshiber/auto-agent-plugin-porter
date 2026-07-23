// CI for <MARKETPLACE_NAME>-marketplace. This is the real server-side merge gate —
// local git hooks are bypassable (--no-verify) and absent for web edits.
//
// NOTE: Bitbucket *Server* ignores bitbucket-pipelines.yml — this Jenkinsfile
// is the CI. Keep the explicit dep install (jsonschema/pytest + shellcheck/jq/
// git) so the gates cannot silently WARN/SKIP.
//
// Runs in a python:3.11-slim container so the toolchain is deterministic:
// deps are installed explicitly (not skipped), then the three gates run in
// order. The gates would otherwise silently WARN/SKIP without jsonschema /
// pytest / shellcheck — installing them here closes that hole.
//
// NOTE: this pipeline is plain declarative Groovy — it does NOT capture any
// shared-library class state inside step closures, so the CPS closure-field
// trap does not apply here.
pipeline {
    agent {
        docker {
            image 'python:3.11-slim'
            // The node must be Docker-capable. Set this label to a
            // Docker-capable agent pool on the controller this repo builds on;
            // without a matching label the job can queue forever waiting for
            // any random executor.
            label '<DOCKER_CAPABLE_AGENT_LABEL>'
        }
    }

    options {
        timeout(time: 5, unit: 'MINUTES')
        timestamps()
        disableConcurrentBuilds()
    }

    stages {
        stage('Install deps') {
            steps {
                sh '''
                    set -eux
                    apt-get update
                    # git is required by validate.sh's whole-repo secrets scan
                    # (git ls-files); the slim image does not ship it, so the
                    # scan would silently no-op on this server-side merge gate.
                    apt-get install -y --no-install-recommends shellcheck jq git
                    pip install --no-cache-dir 'jsonschema>=4.21,<5' 'pytest>=8,<9' 'PyYAML>=6,<7'
                '''
            }
        }
        stage('Validate') {
            steps {
                sh './scripts/validate.sh'
            }
        }
        stage('Validate JSON') {
            steps {
                sh './scripts/validate-json.sh'
            }
        }
        stage('Test') {
            steps {
                sh './scripts/test.sh'
            }
        }
    }

    post {
        always {
            cleanWs()
        }
    }
}
