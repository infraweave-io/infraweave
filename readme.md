
# Terraform CodeBuild CRD

The idea is to create a Kubernetes controller which takes definitions of terraform modules and deploys them using CodeBuild.



## Flow


## Example of CRD

## Setting up workflow

### GitHub

Using Github actions it is possible to sync to the repo:

```yaml
name: Mirror to CodeCommit

on: [push]

jobs:
  mirror:
    runs-on: ubuntu-latest

    steps:
    - name: Checkout code
      uses: actions/checkout@v2

    - name: Configure AWS Credentials
      uses: aws-actions/configure-aws-credentials@v1
      with:
        aws-access-key-id: ${{ secrets.AWS_ACCESS_KEY_ID }}
        aws-secret-access-key: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
        aws-region: us-east-1
        role-to-assume: ARN_OF_YOUR_IAM_ROLE
        role-duration-seconds: 1200 # Optional: Duration for the assumed role credentials

    - name: Push to CodeCommit
      run: |
        # Configure git settings
        git config --global credential.helper '!aws codecommit credential-helper $@'
        git config --global credential.UseHttpPath true

        # Code to push to CodeCommit
        git remote add codecommit YOUR_CODECOMMIT_REPO_URL
        git push codecommit main
```

### Gitlab

The easiest approach is to mirror your repo, which requires setting up SSH keys or long-term credentials to push.

If that is not an alternative, it is possible to sync the current branch (with force) to the repo.

```yaml
stages:
  - mirror

get_aws_credentials_and_push:
  stage: mirror
  image: amazon/aws-cli
  before_script:
    - git config --global credential.helper '!aws codecommit credential-helper $@'
    - git config --global credential.UseHttpPath true
  script:
    - $(aws sts assume-role --role-arn "arn:aws:iam::YOUR_AWS_ACCOUNT_ID:role/YOUR_ROLE_NAME" --role-session-name "GitLabCodeCommitSession" --query 'Credentials.[AccessKeyId,SecretAccessKey,SessionToken]' --output text | awk '{ print "export AWS_ACCESS_KEY_ID="$1"\nexport AWS_SECRET_ACCESS_KEY="$2"\nexport AWS_SESSION_TOKEN="$3 }')
    - git remote add codecommit https://git-codecommit.YOUR_AWS_REGION.amazonaws.com/v1/repos/YOUR_REPO_NAME
    - git push codecommit --all
  only:
    - master
    ```