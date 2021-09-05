# bkt - CLI tool to download and upload files from / to s3 compatible storage.

You can install this tool with the following commands:

```
curl -L https://github.com/YJChan/bkt/releases/download/v0.1.0/bkt-linux-amd64 > bkt

mkdir -p ~/bin && mkdir -p $HOME/.bkt/bin && mv ./bkt $HOME/.bkt/bin

chmod +x $HOME/.bkt/bin/bkt

ln -s $HOME/.bkt/bin/bkt ~/bin/bkt

source ~/.profile
```

Sample screen: 
```
bkt 0.1.0

USAGE:
    bkt [FLAGS] [OPTIONS] <action>

FLAGS:
    -h, --help         Prints help information
    -V, --version      Prints version information
    -v, --verbosity    Pass many times for more log output

OPTIONS:
    -b, --bucket <bucket-name>
            Bucket name, this value will overwrite the bucket name set in config

    -c, --config <config-value> <config-value> <config-value> <config-value> <config-value>
            s3 bucket configuration, sequence as follow:
             access key (s3 access key)
             secret key (s3 secret key)
             bucket (use '-' on bucket if no fix bucket need to define) 
             endpoint (use '-' on endpoint if no endpoint need to configure, default is AWS endpoint) 
             region (use '-' on region if that is s3 compatible services)
    -t, --content-type <content_type>
            Upload file's content type, eg. image/jpeg, application/pdf, etc...

    -d, --destination <destination>
            Location you want to put in s3 bucket

    -f, --folder <folder>
            Recursively upload files in folder to s3 bucket

    -l, --limit <limit>
            Limit number of files to be upload when uploading a folder

    -s, --source <source>
            File you want to upload to s3 bucket


ARGS:
    <action>    allowed arguments are <get> or <put> or <set> or <list-config>

```

